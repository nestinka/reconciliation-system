//! ISO 20022 CAMT.053 (bank-to-customer statement) parsing.
//!
//! Uses quick-xml's pull parser, which does NOT perform DTD processing or
//! external-entity expansion — so XXE / billion-laughs are not reachable. We
//! never implement custom entity resolution.

use crate::{ParsedTxn, Parser, RowError};
use quick_xml::events::Event;
use quick_xml::Reader;
use recon_domain::Direction;

#[derive(Default)]
pub struct Camt053Parser;

#[derive(Default)]
struct EntryAccum {
    amount: Option<String>,
    currency: Option<String>,
    cd_dbt: Option<String>,
    value_date: Option<String>,
    booking_date: Option<String>,
    acct_svcr_ref: Option<String>,
    ntry_ref: Option<String>,
    ustrd: Option<String>,
    addtl: Option<String>,
}

impl Parser for Camt053Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        let text = match std::str::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => return Err(vec![RowError::new(0, "file", "not valid UTF-8")]),
        };
        let mut reader = Reader::from_str(text);
        reader.config_mut().trim_text(true);

        let mut out = Vec::new();
        let mut errors = Vec::new();
        let mut entry_index = 0usize;

        // path is the stack of element local-names; `in_*` flags scope the
        // sub-elements that share generic tags (e.g. <Dt> appears under both
        // <ValDt> and <BookgDt>).
        let mut path: Vec<String> = Vec::new();
        let mut accum: Option<EntryAccum> = None;
        let mut amount_ccy: Option<String> = None;
        let mut last_text = String::new();

        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    if name == "Ntry" {
                        accum = Some(EntryAccum::default());
                        entry_index += 1;
                    }
                    if name == "Amt" {
                        // capture the Ccy attribute
                        amount_ccy = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"Ccy")
                            .and_then(|a| String::from_utf8(a.value.to_vec()).ok());
                    }
                    path.push(name);
                    last_text.clear();
                }
                Ok(Event::Text(t)) => {
                    last_text = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                }
                Ok(Event::End(e)) => {
                    let name = local_name_end(&e);
                    if let Some(acc) = accum.as_mut() {
                        apply_text(acc, &path, &name, &last_text, &mut amount_ccy);
                    }
                    if name == "Ntry" {
                        if let Some(acc) = accum.take() {
                            match finalize(acc, entry_index) {
                                Ok(txn) => out.push(txn),
                                Err(mut errs) => errors.append(&mut errs),
                            }
                        }
                    }
                    path.pop();
                    last_text.clear();
                }
                Ok(Event::Eof) => {
                    // If an Ntry was started but never closed, the XML is truncated.
                    if accum.is_some() {
                        errors.push(RowError::new(entry_index, "xml", "malformed XML: unexpected end of file inside <Ntry>"));
                    }
                    break;
                }
                Err(e) => {
                    errors.push(RowError::new(entry_index, "xml", format!("malformed XML: {e}")));
                    break;
                }
                _ => {}
            }
        }

        if errors.is_empty() {
            Ok(out)
        } else {
            Err(errors)
        }
    }
}

fn local_name(e: &quick_xml::events::BytesStart) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}
fn local_name_end(e: &quick_xml::events::BytesEnd) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

fn parent(path: &[String]) -> &str {
    // path still contains the element we're closing as the last item.
    if path.len() >= 2 { path[path.len() - 2].as_str() } else { "" }
}

fn apply_text(
    acc: &mut EntryAccum,
    path: &[String],
    name: &str,
    text: &str,
    amount_ccy: &mut Option<String>,
) {
    let p = parent(path);
    match name {
        "Amt" => {
            acc.amount = Some(text.to_string());
            acc.currency = amount_ccy.take();
        }
        "CdtDbtInd" => acc.cd_dbt = Some(text.to_string()),
        "Dt" if p == "ValDt" => acc.value_date = Some(text.to_string()),
        "Dt" if p == "BookgDt" => acc.booking_date = Some(text.to_string()),
        "AcctSvcrRef" => acc.acct_svcr_ref = Some(text.to_string()),
        "NtryRef" => acc.ntry_ref = Some(text.to_string()),
        "Ustrd" => acc.ustrd = Some(text.to_string()),
        "AddtlNtryInf" => acc.addtl = Some(text.to_string()),
        _ => {}
    }
}

fn finalize(acc: EntryAccum, idx: usize) -> Result<ParsedTxn, Vec<RowError>> {
    let mut errs = Vec::new();

    let external_ref = acc
        .acct_svcr_ref
        .or(acc.ntry_ref)
        .filter(|s| !s.is_empty());
    if external_ref.is_none() {
        errs.push(RowError::new(idx, "externalRef", "missing AcctSvcrRef/NtryRef"));
    }
    let value_date = acc.value_date.filter(|s| !s.is_empty());
    if value_date.is_none() {
        errs.push(RowError::new(idx, "valueDate", "missing ValDt/Dt"));
    }
    let direction = match acc.cd_dbt.as_deref() {
        Some("DBIT") => Some(Direction::Debit),
        Some("CRDT") => Some(Direction::Credit),
        _ => {
            errs.push(RowError::new(idx, "direction", "missing/invalid CdtDbtInd"));
            None
        }
    };
    let amount_minor = match acc.amount.as_deref() {
        Some(a) => match crate::money::parse_decimal_to_minor(a) {
            Ok(v) => Some(v.abs()),
            Err(m) => {
                errs.push(RowError::new(idx, "amount", m));
                None
            }
        },
        None => {
            errs.push(RowError::new(idx, "amount", "missing Amt"));
            None
        }
    };

    if !errs.is_empty() {
        return Err(errs);
    }

    let value_date = value_date.unwrap();
    let posted_at = acc
        .booking_date
        .filter(|s| !s.is_empty())
        .map(|d| if d.contains('T') { d } else { format!("{d}T00:00:00Z") });

    Ok(ParsedTxn {
        external_ref: external_ref.unwrap(),
        value_date,
        posted_at,
        amount_minor: amount_minor.unwrap(),
        currency: acc.currency.filter(|s| !s.is_empty()),
        direction: direction.unwrap(),
        counterparty: None,
        description: acc.ustrd.or(acc.addtl).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
  <BkToCstmrStmt>
    <Stmt>
      <Ntry>
        <Amt Ccy="GBP">125.00</Amt>
        <CdtDbtInd>DBIT</CdtDbtInd>
        <BookgDt><Dt>2026-05-10</Dt></BookgDt>
        <ValDt><Dt>2026-05-10</Dt></ValDt>
        <NtryDtls><TxDtls>
          <Refs><AcctSvcrRef>REF-001</AcctSvcrRef></Refs>
          <RmtInf><Ustrd>Invoice 4001</Ustrd></RmtInf>
        </TxDtls></NtryDtls>
      </Ntry>
      <Ntry>
        <Amt Ccy="GBP">90.50</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <ValDt><Dt>2026-05-11</Dt></ValDt>
        <NtryRef>REF-002</NtryRef>
        <AddtlNtryInf>Customer payment</AddtlNtryInf>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;

    #[test]
    fn parses_two_entries() {
        let txns = Camt053Parser.parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(txns.len(), 2);

        assert_eq!(txns[0].external_ref, "REF-001");
        assert_eq!(txns[0].amount_minor, 12500);
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].value_date, "2026-05-10");
        assert_eq!(txns[0].currency.as_deref(), Some("GBP"));
        assert_eq!(txns[0].posted_at.as_deref(), Some("2026-05-10T00:00:00Z"));
        assert_eq!(txns[0].description, "Invoice 4001");

        assert_eq!(txns[1].external_ref, "REF-002");
        assert_eq!(txns[1].direction, Direction::Credit);
        assert_eq!(txns[1].amount_minor, 9050);
        assert_eq!(txns[1].posted_at, None);
        assert_eq!(txns[1].description, "Customer payment");
    }

    #[test]
    fn entry_missing_required_fields_errors() {
        let xml = r#"<Document><Stmt><Ntry>
            <Amt Ccy="GBP">10.00</Amt>
            <ValDt><Dt>2026-05-10</Dt></ValDt>
          </Ntry></Stmt></Document>"#;
        // No CdtDbtInd, no ref -> two errors, nothing returned.
        let errs = Camt053Parser.parse(xml.as_bytes()).unwrap_err();
        let fields: Vec<&str> = errs.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"direction"));
        assert!(fields.contains(&"externalRef"));
    }

    #[test]
    fn malformed_xml_errors() {
        let xml = "<Document><Ntry><Amt>oops"; // unclosed
        assert!(Camt053Parser.parse(xml.as_bytes()).is_err());
    }
}
