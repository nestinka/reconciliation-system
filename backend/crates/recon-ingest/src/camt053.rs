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
    cdtr_bic: Option<String>,
    cdtr_account: Option<String>,
    dbtr_bic: Option<String>,
    dbtr_account: Option<String>,
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
                    match t.unescape() {
                        Ok(c) => last_text = c.into_owned(),
                        Err(e) => {
                            errors.push(RowError::new(entry_index, "xml", format!("malformed XML: {e}")));
                            break;
                        }
                    }
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

fn path_contains(path: &[String], target: &str) -> bool {
    path.iter().any(|s| s == target)
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
        "DtTm" if p == "ValDt" => acc.value_date = Some(text.split('T').next().unwrap_or(text).to_string()),
        "DtTm" if p == "BookgDt" => acc.booking_date = Some(text.to_string()),
        "AcctSvcrRef" => acc.acct_svcr_ref = Some(text.to_string()),
        "NtryRef" => acc.ntry_ref = Some(text.to_string()),
        "Ustrd" => acc.ustrd = Some(text.to_string()),
        "AddtlNtryInf" => acc.addtl = Some(text.to_string()),
        "BIC" | "BICFI" => {
            // Counterparty BIC lives under <CdtrAgt>/<FinInstnId>/<BIC|BICFI>
            // or <DbtrAgt>/.../<BIC|BICFI>. Skip the bank's own account agent.
            let raw = text.trim().to_uppercase();
            if !raw.is_empty() {
                if path_contains(path, "CdtrAgt") && acc.cdtr_bic.is_none() {
                    acc.cdtr_bic = Some(raw);
                } else if path_contains(path, "DbtrAgt") && acc.dbtr_bic.is_none() {
                    acc.dbtr_bic = Some(raw);
                }
            }
        }
        "IBAN" => {
            let raw = text.trim().to_string();
            if !raw.is_empty() {
                if path_contains(path, "CdtrAcct") && acc.cdtr_account.is_none() {
                    acc.cdtr_account = Some(raw);
                } else if path_contains(path, "DbtrAcct") && acc.dbtr_account.is_none() {
                    acc.dbtr_account = Some(raw);
                }
            }
        }
        "Id" => {
            // Non-IBAN account is <CdtrAcct>/<Id>/<Othr>/<Id> — capture only when
            // we're inside an <Othr> and the enclosing *Acct is CdtrAcct or DbtrAcct.
            let raw = text.trim().to_string();
            if path_contains(path, "Othr") && !raw.is_empty() {
                if path_contains(path, "CdtrAcct") && acc.cdtr_account.is_none() {
                    acc.cdtr_account = Some(raw);
                } else if path_contains(path, "DbtrAcct") && acc.dbtr_account.is_none() {
                    acc.dbtr_account = Some(raw);
                }
            }
        }
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

    let direction = direction.unwrap();
    let value_date = value_date.unwrap();
    let amount_minor = amount_minor.unwrap();
    let external_ref = external_ref.unwrap();

    let (counterparty_bic, counterparty_account) = match direction {
        // CRDT entry: our account received money → counterparty is the payer (Dbtr).
        // Prefer Dbtr side; fall back to Cdtr if Dbtr is empty (some banks only
        // include the counterparty side regardless of which is "our" side).
        Direction::Credit => (
            acc.dbtr_bic.or(acc.cdtr_bic),
            acc.dbtr_account.or(acc.cdtr_account),
        ),
        // DBIT entry: our account paid → counterparty is the receiver (Cdtr).
        Direction::Debit => (
            acc.cdtr_bic.or(acc.dbtr_bic),
            acc.cdtr_account.or(acc.dbtr_account),
        ),
    };

    let posted_at = acc
        .booking_date
        .filter(|s| !s.is_empty())
        .map(|d| if d.contains('T') { d } else { format!("{d}T00:00:00Z") });

    Ok(ParsedTxn {
        external_ref,
        value_date,
        posted_at,
        amount_minor,
        currency: acc.currency.filter(|s| !s.is_empty()),
        direction,
        counterparty: None,
        description: acc.ustrd.or(acc.addtl).unwrap_or_default(),
        counterparty_bic,
        counterparty_account,
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

    #[test]
    fn parses_dtdtm_value_date() {
        let xml = r#"<Document><Stmt><Ntry>
            <Amt Ccy="USD">42.00</Amt>
            <CdtDbtInd>CRDT</CdtDbtInd>
            <ValDt><DtTm>2026-05-10T09:30:00Z</DtTm></ValDt>
            <NtryRef>REF-DtTm</NtryRef>
          </Ntry></Stmt></Document>"#;
        let txns = Camt053Parser.parse(xml.as_bytes()).unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].value_date, "2026-05-10");
    }

    #[test]
    fn credit_entry_extracts_counterparty_bic_and_account() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
 <BkToCstmrStmt>
  <Stmt>
   <Id>S1</Id>
   <Ntry>
    <Amt Ccy="EUR">100.00</Amt>
    <CdtDbtInd>CRDT</CdtDbtInd>
    <BookgDt><Dt>2026-01-01</Dt></BookgDt>
    <ValDt><Dt>2026-01-01</Dt></ValDt>
    <AcctSvcrRef>R1</AcctSvcrRef>
    <NtryDtls>
     <TxDtls>
      <Refs><EndToEndId>R1</EndToEndId></Refs>
      <RltdPties>
       <Cdtr><Nm>Receiver</Nm></Cdtr>
       <CdtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></CdtrAcct>
      </RltdPties>
      <RltdAgts>
       <CdtrAgt><FinInstnId><BIC>DEUTDEFF</BIC></FinInstnId></CdtrAgt>
      </RltdAgts>
     </TxDtls>
    </NtryDtls>
   </Ntry>
  </Stmt>
 </BkToCstmrStmt>
</Document>"#;
        let txns = Camt053Parser.parse(xml.as_bytes()).unwrap();
        assert_eq!(txns.len(), 1);
        // CRDT entry with only Cdtr* side populated → falls back to Cdtr side
        // (Dbtr is empty so preferred branch is empty).
        assert_eq!(txns[0].counterparty_bic.as_deref(), Some("DEUTDEFF"));
        assert_eq!(
            txns[0].counterparty_account.as_deref(),
            Some("DE89370400440532013000")
        );
    }

    #[test]
    fn debit_entry_extracts_counterparty_from_dbtr_branches() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document><Stmt>
  <Ntry>
   <Amt Ccy="EUR">50.00</Amt>
   <CdtDbtInd>DBIT</CdtDbtInd>
   <BookgDt><Dt>2026-01-01</Dt></BookgDt>
   <ValDt><Dt>2026-01-01</Dt></ValDt>
   <AcctSvcrRef>R2</AcctSvcrRef>
   <NtryDtls><TxDtls>
    <Refs><EndToEndId>R2</EndToEndId></Refs>
    <RltdPties>
     <Dbtr><Nm>Payer</Nm></Dbtr>
     <DbtrAcct><Id><IBAN>FR1420041010050500013M02606</IBAN></Id></DbtrAcct>
    </RltdPties>
    <RltdAgts>
     <DbtrAgt><FinInstnId><BIC>BNPAFRPP</BIC></FinInstnId></DbtrAgt>
    </RltdAgts>
   </TxDtls></NtryDtls>
  </Ntry>
</Stmt></Document>"#;
        let txns = Camt053Parser.parse(xml.as_bytes()).unwrap();
        // DBIT entry with only Dbtr* side populated → falls back to Dbtr side.
        assert_eq!(txns[0].counterparty_bic.as_deref(), Some("BNPAFRPP"));
        assert_eq!(
            txns[0].counterparty_account.as_deref(),
            Some("FR1420041010050500013M02606")
        );
    }

    #[test]
    fn missing_rltd_pties_leaves_counterparty_fields_none() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document><Stmt>
  <Ntry>
   <Amt Ccy="EUR">10.00</Amt>
   <CdtDbtInd>CRDT</CdtDbtInd>
   <BookgDt><Dt>2026-01-01</Dt></BookgDt>
   <ValDt><Dt>2026-01-01</Dt></ValDt>
   <AcctSvcrRef>R3</AcctSvcrRef>
   <NtryDtls><TxDtls><Refs><EndToEndId>R3</EndToEndId></Refs></TxDtls></NtryDtls>
  </Ntry>
</Stmt></Document>"#;
        let txns = Camt053Parser.parse(xml.as_bytes()).unwrap();
        assert!(txns[0].counterparty_bic.is_none());
        assert!(txns[0].counterparty_account.is_none());
    }

    #[test]
    fn non_iban_account_via_othr_id_is_picked_up() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document><Stmt>
  <Ntry>
   <Amt Ccy="USD">10.00</Amt>
   <CdtDbtInd>CRDT</CdtDbtInd>
   <BookgDt><Dt>2026-01-01</Dt></BookgDt>
   <ValDt><Dt>2026-01-01</Dt></ValDt>
   <AcctSvcrRef>R4</AcctSvcrRef>
   <NtryDtls><TxDtls>
    <Refs><EndToEndId>R4</EndToEndId></Refs>
    <RltdPties>
     <Cdtr><Nm>US Vendor</Nm></Cdtr>
     <CdtrAcct><Id><Othr><Id>1234567890</Id></Othr></Id></CdtrAcct>
    </RltdPties>
   </TxDtls></NtryDtls>
  </Ntry>
</Stmt></Document>"#;
        let txns = Camt053Parser.parse(xml.as_bytes()).unwrap();
        assert_eq!(txns[0].counterparty_account.as_deref(), Some("1234567890"));
        assert!(txns[0].counterparty_bic.is_none());
    }
}
