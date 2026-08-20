//! Sending a letter.
//!
//! tezgah asks a host for five things and a mailer is not one of them: the
//! crate never sends mail, so this is entirely the product's — which is why
//! `docs/hosting.md` gained no sixth port for it.
//!
//! Unset, unbound. Everything that needs a letter checks for one first and
//! says plainly that it cannot, the same way the callback route is not
//! mounted without a secret. A shop with no SMTP is a shop where an owner
//! sets a colleague's password by hand, which is what it did before this
//! existed and is still a working way to run one.
//!
//! One transport, no queue, no templates. A letter is sent in the request
//! that caused it and its failure is that request's — an invitation that
//! could not be sent is not an invitation, and telling the owner so
//! immediately beats a row in a table nobody reads.

use std::sync::Arc;

use lettre::message::header::ContentType;
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::{AsyncTransport, Message, Tokio1Executor};

#[derive(Clone)]
pub struct Mailer {
    transport: Arc<AsyncSmtpTransport<Tokio1Executor>>,
    from: Arc<str>,
}

/// Written out rather than derived: the URL carries the password.
impl std::fmt::Debug for Mailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailer").field("from", &self.from).finish()
    }
}

impl Mailer {
    /// `url` is lettre's own: `smtps://user:pass@host:465`, or `smtp://` with
    /// `tls=required` for STARTTLS. Parsing it here rather than at startup
    /// would make a typo a runtime failure on the first invitation; `main`
    /// builds this before it serves.
    pub fn new(url: &str, from: &str) -> tezgah::Result<Mailer> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|err| tezgah::Error::invalid(format!("TEZGAH_SMTP_URL: {err}")))?
            .build();

        Ok(Mailer {
            transport: Arc::new(transport),
            from: Arc::from(from),
        })
    }

    /// Plain text on purpose. HTML mail is a rendering problem, a tracking
    /// problem and a phishing lesson at once, and nothing this binary sends
    /// needs more than a sentence and a link.
    pub async fn send(&self, to: &str, subject: &str, body: &str) -> tezgah::Result<()> {
        let message = Message::builder()
            .from(
                self.from
                    .parse()
                    .map_err(|_| tezgah::Error::invalid("TEZGAH_MAIL_FROM is not an address"))?,
            )
            .to(to
                .parse()
                .map_err(|_| tezgah::Error::invalid("that is not an e-mail address"))?)
            .subject(subject.to_owned())
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_owned())
            .map_err(|err| {
                tezgah::Error::invalid(format!("a letter that will not build: {err}"))
            })?;

        self.transport.send(message).await.map_err(|err| {
            // The address is in the error and the body is not: a bounced
            // invitation should say who it was for, and never what was in it.
            tezgah::Error::invalid(format!("could not send to {to}: {err}"))
        })?;

        Ok(())
    }
}
