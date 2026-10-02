//! S2 live smoke: dial mail4agent from the hatchery harness crate via MailDial.
//!
//! Env:
//!   MAIL4AGENT_URL   (default http://127.0.0.1:18301)
//!   MAIL4AGENT_KEY   path to operator (or participant) bearer file
//! Never prints secrets.

use hatchery_harness_service::mail::{MailDial, HATCHERY_MAIL_DEFAULT_URL};
use mail4agent_api::ParticipantId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("MAIL4AGENT_URL").unwrap_or_else(|_| HATCHERY_MAIL_DEFAULT_URL.to_string());
    let key = std::env::var("MAIL4AGENT_KEY").map_err(|_| "MAIL4AGENT_KEY path required")?;

    println!("s2-mail: health_at {url}");
    let health = MailDial::health_at(&url).await?;
    if !health.ok || health.service != "mail4agent" {
        return Err(format!("health not ok: {health:?}").into());
    }
    println!(
        "s2-mail: health ok service={} version={:?} uptime_secs={}",
        health.service, health.version, health.uptime_secs
    );

    println!("s2-mail: dial with key file (redacted)");
    let dial = MailDial::connect_with_key_file(&url, &key)?;

    let me = dial.whoami().await?;
    let account = me
        .address
        .account()
        .cloned()
        .ok_or("whoami address has no account")?;
    println!("s2-mail: whoami address={} account={}", me.address, account);

    let dir = dial.directory().await?;
    println!(
        "s2-mail: directory participants={} rooms={}",
        dir.participants.len(),
        dir.rooms.len()
    );

    // Register a disposable peer so create/list path is exercised (ignore if exists).
    let peer = ParticipantId::new("hatchery-s2")?;
    match dial
        .register_peer(peer.clone(), Some("S2 smoke peer".into()))
        .await
    {
        Ok(_) => println!("s2-mail: registered peer hatchery-s2 (secret not printed)"),
        Err(e) => println!("s2-mail: peer register skipped ({e})"),
    }

    // Self-send so inbox list is observable on this same session/account.
    let sent = dial
        .send_direct(account.clone(), "s2-smoke", "hatchery MailDial smoke body")
        .await?;
    println!(
        "s2-mail: send ok message_id={} from={}",
        sent.message_id, sent.from
    );

    let page = dial.list_inbox().await?;
    println!(
        "s2-mail: inbox messages={} unread={}",
        page.messages.len(),
        page.unread
    );
    if page.messages.is_empty() {
        return Err("inbox empty after self-send".into());
    }
    let saw = page
        .messages
        .iter()
        .any(|m| m.message_id == sent.message_id);
    if !saw {
        return Err("sent message_id not in inbox page".into());
    }

    println!("s2-mail: PASS");
    Ok(())
}
