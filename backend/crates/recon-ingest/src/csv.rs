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
    #[serde(default)]
    pub counterparty_bic: Option<ColRef>,
    #[serde(default)]
    pub counterparty_account: Option<ColRef>,
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
            // Row number presented to users is the 1-based file line. With a
            // header on line 1, the reader has already consumed it, so the
            // first data record (i=0) is line 2; without a header it is line 1.
            let row = if self.mapping.has_header { i + 2 } else { i + 1 };
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

        let counterparty_bic = match &self.mapping.counterparty_bic {
            None => None,
            Some(c) => {
                let raw = match self.get(record, headers, c) {
                    Ok(v) => v,
                    Err(m) => {
                        errs.push(RowError::new(row, "counterparty_bic", m));
                        ""
                    }
                };
                let cleaned = raw.trim().to_uppercase();
                if cleaned.is_empty() { None } else { Some(cleaned) }
            }
        };
        let counterparty_account = match &self.mapping.counterparty_account {
            None => None,
            Some(c) => {
                let raw = match self.get(record, headers, c) {
                    Ok(v) => v,
                    Err(m) => {
                        errs.push(RowError::new(row, "counterparty_account", m));
                        ""
                    }
                };
                let cleaned = raw.trim().to_string();
                if cleaned.is_empty() { None } else { Some(cleaned) }
            }
        };

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
            counterparty_bic,
            counterparty_account,
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
                // Resolve both columns, propagating resolution errors.
                let d_raw = match self.get(record, headers, debit) {
                    Ok(v) => v.trim().to_string(),
                    Err(m) => {
                        errs.push(RowError::new(row, "amount", m));
                        return (None, None);
                    }
                };
                let c_raw = match self.get(record, headers, credit) {
                    Ok(v) => v.trim().to_string(),
                    Err(m) => {
                        errs.push(RowError::new(row, "amount", m));
                        return (None, None);
                    }
                };
                // Parse each column once, reusing the result.
                let d_val = if d_raw.is_empty() {
                    None
                } else {
                    match parse_decimal_to_minor(&d_raw) {
                        Ok(v) => Some(v),
                        Err(m) => {
                            errs.push(RowError::new(row, "amount", m));
                            return (None, None);
                        }
                    }
                };
                let c_val = if c_raw.is_empty() {
                    None
                } else {
                    match parse_decimal_to_minor(&c_raw) {
                        Ok(v) => Some(v),
                        Err(m) => {
                            errs.push(RowError::new(row, "amount", m));
                            return (None, None);
                        }
                    }
                };
                let d_has = d_val.map(|v| v != 0).unwrap_or(false);
                let c_has = c_val.map(|v| v != 0).unwrap_or(false);
                match (d_has, c_has) {
                    (true, false) => (Some(d_val.unwrap().abs()), Some(Direction::Debit)),
                    (false, true) => (Some(c_val.unwrap().abs()), Some(Direction::Credit)),
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
            counterparty_bic: None,
            counterparty_account: None,
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
            counterparty_bic: None,
            counterparty_account: None,
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
        assert_eq!(errs[0].row, 3); // R2 is file line 3 (header=1, R1=2, R2=3)
        assert_eq!(errs[0].field, "valueDate");
        assert_eq!(errs[1].row, 4); // R3 is file line 4
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
            counterparty_bic: None,
            counterparty_account: None,
        };
        let csv = "R1,2026-05-10,Coffee\n";
        let errs = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].field, "amount");
    }

    #[test]
    fn debit_credit_out_of_range_column_surfaces_error() {
        let mapping = CsvMapping {
            has_header: false,
            delimiter: b',',
            external_ref: ColRef::Index(0),
            value_date: ColRef::Index(1),
            date_format: "%Y-%m-%d".into(),
            // debit column index 9 is out of range for a 3-column row
            amount: AmountMapping::DebitCredit {
                debit: ColRef::Index(9),
                credit: ColRef::Index(2),
            },
            description: ColRef::Index(2),
            currency: None,
            counterparty: None,
            counterparty_bic: None,
            counterparty_account: None,
        };
        let csv = "R1,2026-05-10,40.00\n";
        let errs = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].field, "amount");
        assert!(errs[0].message.contains("out of range"), "expected 'out of range' in: {}", errs[0].message);
    }

    #[test]
    fn counterparty_bic_and_account_columns_extracted_and_bic_uppercased() {
        let mapping = CsvMapping {
            has_header: true,
            delimiter: b',',
            external_ref: ColRef::Index(0),
            value_date: ColRef::Index(1),
            date_format: "%Y-%m-%d".into(),
            amount: AmountMapping::Signed { column: ColRef::Index(2), debit_when_negative: true },
            description: ColRef::Index(3),
            currency: None,
            counterparty: None,
            counterparty_bic: Some(ColRef::Index(4)),
            counterparty_account: Some(ColRef::Index(5)),
        };
        let csv = "ref,date,amount,desc,bic,acc\n\
                   R1,2026-01-01,100.00,Test 1,deutdeff,DE89370400440532013000\n\
                   R2,2026-01-02,200.00,Test 2,,\n";
        let txns = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].counterparty_bic.as_deref(), Some("DEUTDEFF")); // uppercased
        assert_eq!(
            txns[0].counterparty_account.as_deref(),
            Some("DE89370400440532013000")
        );
        // Row 2 has empty values → None.
        assert!(txns[1].counterparty_bic.is_none());
        assert!(txns[1].counterparty_account.is_none());
    }

    #[test]
    fn counterparty_columns_default_none_when_mapping_omits_them() {
        let mapping = CsvMapping {
            has_header: true,
            delimiter: b',',
            external_ref: ColRef::Index(0),
            value_date: ColRef::Index(1),
            date_format: "%Y-%m-%d".into(),
            amount: AmountMapping::Signed { column: ColRef::Index(2), debit_when_negative: true },
            description: ColRef::Index(3),
            currency: None,
            counterparty: None,
            counterparty_bic: None,
            counterparty_account: None,
        };
        let csv = "ref,date,amount,desc\nR1,2026-01-01,100.00,Test\n";
        let txns = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap();
        assert!(txns[0].counterparty_bic.is_none());
        assert!(txns[0].counterparty_account.is_none());
    }
}
