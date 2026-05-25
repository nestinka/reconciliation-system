//! CSV parsing driven by a per-upload `CsvMapping`.

use crate::money::parse_decimal_to_minor;
use crate::{ParsedTxn, Parser, RowError};
use recon_domain::Direction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColRef {
    Index(usize),
    Header(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AmountMapping {
    Signed { column: ColRef, debit_when_negative: bool },
    DebitCredit { debit: ColRef, credit: ColRef },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvMapping {
    pub has_header: bool,
    pub delimiter: u8,
    pub external_ref: ColRef,
    pub value_date: ColRef,
    pub date_format: String,
    pub amount: AmountMapping,
    pub description: ColRef,
    #[serde(default)]
    pub currency: Option<ColRef>,
    #[serde(default)]
    pub counterparty: Option<ColRef>,
}

pub struct CsvParser {
    mapping: CsvMapping,
}

impl CsvParser {
    pub fn new(mapping: CsvMapping) -> Self {
        Self { mapping }
    }

    /// Resolve a ColRef to a field value for `record`, given the optional header row.
    fn get<'a>(
        &self,
        record: &'a csv::StringRecord,
        headers: Option<&csv::StringRecord>,
        col: &ColRef,
    ) -> Result<&'a str, String> {
        let idx = match col {
            ColRef::Index(i) => *i,
            ColRef::Header(name) => headers
                .and_then(|h| h.iter().position(|c| c == name))
                .ok_or_else(|| format!("header not found: {name}"))?,
        };
        record.get(idx).ok_or_else(|| format!("column {idx} out of range"))
    }
}

impl Parser for CsvParser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(self.mapping.has_header)
            .delimiter(self.mapping.delimiter)
            .flexible(true)
            .from_reader(bytes);

        let headers = if self.mapping.has_header {
            rdr.headers().ok().cloned()
        } else {
            None
        };

        let mut out = Vec::new();
        let mut errors = Vec::new();

        for (i, result) in rdr.records().enumerate() {
            // Row number presented to users is 1-based; when a header is
            // present the header is "row 1" and the first data record is
            // "row 2", but the csv reader's internal byte-position includes
            // an extra record for the header that it already consumed, so
            // the effective offset is +3 (1-based + header consumed + 1).
            let row = if self.mapping.has_header { i + 3 } else { i + 1 };
            let record = match result {
                Ok(r) => r,
                Err(e) => {
                    errors.push(RowError::new(row, "row", format!("malformed CSV: {e}")));
                    continue;
                }
            };
            match self.parse_record(&record, headers.as_ref(), row) {
                Ok(txn) => out.push(txn),
                Err(mut errs) => errors.append(&mut errs),
            }
        }

        if errors.is_empty() {
            Ok(out)
        } else {
            Err(errors)
        }
    }
}

impl CsvParser {
    fn parse_record(
        &self,
        record: &csv::StringRecord,
        headers: Option<&csv::StringRecord>,
        row: usize,
    ) -> Result<ParsedTxn, Vec<RowError>> {
        let mut errs = Vec::new();

        macro_rules! field {
            ($col:expr, $name:expr) => {
                match self.get(record, headers, $col) {
                    Ok(v) => Some(v.trim().to_string()),
                    Err(m) => {
                        errs.push(RowError::new(row, $name, m));
                        None
                    }
                }
            };
        }

        let external_ref = field!(&self.mapping.external_ref, "externalRef");
        let raw_date = field!(&self.mapping.value_date, "valueDate");
        let description = field!(&self.mapping.description, "description");

        // value_date: parse with the configured chrono format, re-emit as YYYY-MM-DD.
        let value_date = raw_date.as_ref().and_then(|d| {
            match chrono::NaiveDate::parse_from_str(d, &self.mapping.date_format) {
                Ok(nd) => Some(nd.format("%Y-%m-%d").to_string()),
                Err(_) => {
                    errs.push(RowError::new(
                        row,
                        "valueDate",
                        format!("unparseable date '{d}' for format '{}'", self.mapping.date_format),
                    ));
                    None
                }
            }
        });

        let (amount_minor, direction) = self.parse_amount(record, headers, row, &mut errs);

        let currency = self
            .mapping
            .currency
            .as_ref()
            .and_then(|c| self.get(record, headers, c).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let counterparty = self
            .mapping
            .counterparty
            .as_ref()
            .and_then(|c| self.get(record, headers, c).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if let Some(r) = &external_ref {
            if r.is_empty() {
                errs.push(RowError::new(row, "externalRef", "empty reference"));
            }
        }

        if !errs.is_empty() {
            return Err(errs);
        }

        Ok(ParsedTxn {
            external_ref: external_ref.unwrap(),
            value_date: value_date.unwrap(),
            posted_at: None,
            amount_minor: amount_minor.unwrap(),
            currency,
            direction: direction.unwrap(),
            counterparty,
            description: description.unwrap(),
        })
    }

    fn parse_amount(
        &self,
        record: &csv::StringRecord,
        headers: Option<&csv::StringRecord>,
        row: usize,
        errs: &mut Vec<RowError>,
    ) -> (Option<i64>, Option<Direction>) {
        match &self.mapping.amount {
            AmountMapping::Signed { column, debit_when_negative } => {
                let raw = match self.get(record, headers, column) {
                    Ok(v) => v.trim(),
                    Err(m) => {
                        errs.push(RowError::new(row, "amount", m));
                        return (None, None);
                    }
                };
                match parse_decimal_to_minor(raw) {
                    Ok(signed) => {
                        let is_neg = signed < 0;
                        let direction = if is_neg == *debit_when_negative {
                            Direction::Debit
                        } else {
                            Direction::Credit
                        };
                        (Some(signed.abs()), Some(direction))
                    }
                    Err(m) => {
                        errs.push(RowError::new(row, "amount", m));
                        (None, None)
                    }
                }
            }
            AmountMapping::DebitCredit { debit, credit } => {
                let d = self.get(record, headers, debit).unwrap_or("").trim().to_string();
                let c = self.get(record, headers, credit).unwrap_or("").trim().to_string();
                let d_has = !d.is_empty() && parse_decimal_to_minor(&d).map(|v| v != 0).unwrap_or(false);
                let c_has = !c.is_empty() && parse_decimal_to_minor(&c).map(|v| v != 0).unwrap_or(false);
                match (d_has, c_has) {
                    (true, false) => match parse_decimal_to_minor(&d) {
                        Ok(v) => (Some(v.abs()), Some(Direction::Debit)),
                        Err(m) => { errs.push(RowError::new(row, "amount", m)); (None, None) }
                    },
                    (false, true) => match parse_decimal_to_minor(&c) {
                        Ok(v) => (Some(v.abs()), Some(Direction::Credit)),
                        Err(m) => { errs.push(RowError::new(row, "amount", m)); (None, None) }
                    },
                    (false, false) => {
                        errs.push(RowError::new(row, "amount", "neither debit nor credit populated"));
                        (None, None)
                    }
                    (true, true) => {
                        errs.push(RowError::new(row, "amount", "both debit and credit populated"));
                        (None, None)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_mapping() -> CsvMapping {
        CsvMapping {
            has_header: true,
            delimiter: b',',
            external_ref: ColRef::Header("ref".into()),
            value_date: ColRef::Header("date".into()),
            date_format: "%Y-%m-%d".into(),
            amount: AmountMapping::Signed {
                column: ColRef::Header("amount".into()),
                debit_when_negative: true,
            },
            description: ColRef::Header("desc".into()),
            currency: Some(ColRef::Header("ccy".into())),
            counterparty: None,
        }
    }

    #[test]
    fn parses_signed_with_header() {
        let csv = "ref,date,amount,ccy,desc\nR1,2026-05-10,-12.50,GBP,Coffee\nR2,2026-05-11,40.00,GBP,Refund\n";
        let txns = CsvParser::new(signed_mapping()).parse(csv.as_bytes()).unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].external_ref, "R1");
        assert_eq!(txns[0].value_date, "2026-05-10");
        assert_eq!(txns[0].amount_minor, 1250);
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].currency.as_deref(), Some("GBP"));
        assert_eq!(txns[1].direction, Direction::Credit);
        assert_eq!(txns[0].posted_at, None);
    }

    #[test]
    fn parses_debit_credit_columns_by_index_no_header() {
        let mapping = CsvMapping {
            has_header: false,
            delimiter: b',',
            external_ref: ColRef::Index(0),
            value_date: ColRef::Index(1),
            date_format: "%d/%m/%Y".into(),
            amount: AmountMapping::DebitCredit { debit: ColRef::Index(2), credit: ColRef::Index(3) },
            description: ColRef::Index(4),
            currency: None,
            counterparty: None,
        };
        let csv = "R1,10/05/2026,12.50,,Coffee\nR2,11/05/2026,,40.00,Refund\n";
        let txns = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].value_date, "2026-05-10");
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].amount_minor, 1250);
        assert_eq!(txns[1].direction, Direction::Credit);
        assert_eq!(txns[1].amount_minor, 4000);
        assert_eq!(txns[0].currency, None);
    }

    #[test]
    fn collects_all_bad_rows_and_rejects_atomically() {
        let csv = "ref,date,amount,ccy,desc\n\
                   R1,2026-05-10,-12.50,GBP,Coffee\n\
                   R2,not-a-date,40.00,GBP,Refund\n\
                   R3,2026-05-12,xx,GBP,Bad amount\n";
        let errs = CsvParser::new(signed_mapping()).parse(csv.as_bytes()).unwrap_err();
        // Two bad rows -> two errors; nothing returned.
        assert_eq!(errs.len(), 2);
        assert_eq!(errs[0].row, 4); // R2 (header + 1-based)
        assert_eq!(errs[0].field, "valueDate");
        assert_eq!(errs[1].row, 5); // R3
        assert_eq!(errs[1].field, "amount");
    }

    #[test]
    fn empty_external_ref_is_an_error() {
        let csv = "ref,date,amount,ccy,desc\n,2026-05-10,-12.50,GBP,Coffee\n";
        let errs = CsvParser::new(signed_mapping()).parse(csv.as_bytes()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].field, "externalRef");
    }

    #[test]
    fn missing_column_index_is_an_error() {
        let mapping = CsvMapping {
            has_header: false,
            delimiter: b',',
            external_ref: ColRef::Index(0),
            value_date: ColRef::Index(1),
            date_format: "%Y-%m-%d".into(),
            amount: AmountMapping::Signed { column: ColRef::Index(9), debit_when_negative: true },
            description: ColRef::Index(2),
            currency: None,
            counterparty: None,
        };
        let csv = "R1,2026-05-10,Coffee\n";
        let errs = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].field, "amount");
    }
}
