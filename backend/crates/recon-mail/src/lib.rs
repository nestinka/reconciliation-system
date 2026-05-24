use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct Email { pub to: String, pub subject: String, pub body: String }

#[derive(Debug, thiserror::Error)]
pub enum MailError { #[error("smtp: {0}")] Smtp(String) }

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, email: Email) -> Result<(), MailError>;
}

/// Logs the email instead of sending (used when no SMTP is configured).
pub struct LogMailer;
#[async_trait]
impl Mailer for LogMailer {
    async fn send(&self, email: Email) -> Result<(), MailError> {
        tracing::warn!(to = %email.to, subject = %email.subject, "LogMailer (no SMTP): {}", email.body);
        Ok(())
    }
}

/// Sends via SMTP. Mailpit (dev) speaks plaintext SMTP, so we use the dangerous (no-TLS) builder.
pub struct SmtpMailer { host: String, port: u16, from: String }
impl SmtpMailer {
    pub fn new(host: impl Into<String>, port: u16, from: impl Into<String>) -> Self {
        Self { host: host.into(), port, from: from.into() }
    }
}
#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, email: Email) -> Result<(), MailError> {
        use lettre::{Message, AsyncSmtpTransport, Tokio1Executor, AsyncTransport};
        let msg = Message::builder()
            .from(self.from.parse().map_err(|e| MailError::Smtp(format!("{e}")))?)
            .to(email.to.parse().map_err(|e| MailError::Smtp(format!("{e}")))?)
            .subject(email.subject)
            .body(email.body).map_err(|e| MailError::Smtp(format!("{e}")))?;
        let mailer = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.host).port(self.port).build();
        mailer.send(msg).await.map_err(|e| MailError::Smtp(format!("{e}")))?;
        Ok(())
    }
}

/// Test double that captures sent mail. Available in tests and behind the `test-util` feature.
#[cfg(any(test, feature = "test-util"))]
pub mod testing {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    pub struct CapturingMailer { pub sent: Mutex<Vec<Email>> }
    #[async_trait]
    impl Mailer for CapturingMailer {
        async fn send(&self, email: Email) -> Result<(), MailError> {
            self.sent.lock().unwrap().push(email);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn capturing_mailer_records() {
        let m = testing::CapturingMailer::default();
        m.send(Email { to: "a@b.com".into(), subject: "s".into(), body: "hello LINK".into() }).await.unwrap();
        let sent = m.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].body.contains("LINK"));
    }
}
