use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::thread;
use std::time::Duration;

// Notes:
// - Runs continuously
// - Tracks previous height in temp file (.last_height)
// - Sends Telegram message only when:
//   Sync is stuck (.last_status)
//   Sync resumes (block height increases again)

const LAST_HEIGHT_FILE: &str = ".last_height";
const LAST_STATUS_FILE: &str = ".last_status";
const CHECK_INTERVAL: Duration = Duration::from_secs(60);

// Ethereum JSON-RPC response
#[derive(Deserialize)]
struct EthResponse {
    result: String,
}

// ---------------------------------------------------------------------------
// State machine core
// ---------------------------------------------------------------------------

/// The status persisted between loop iterations.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Ok,
    Stuck,
    Down,
}

impl Status {
    pub fn from_str(s: &str) -> Self {
        match s {
            "stuck" => Status::Stuck,
            "down" => Status::Down,
            _ => Status::Ok,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Stuck => "stuck",
            Status::Down => "down",
        }
    }
}

/// Actions the main loop should carry out after a `step()` call.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Send a Telegram notification with this message.
    Notify(String),
    /// Persist the new status.
    SetStatus(Status),
    /// Persist the latest block height.
    SetHeight(i64),
}

/// Pure state-machine step.
///
/// Given the previous persisted state and the result of the latest RPC poll,
/// returns the list of side-effects that should be performed (in order).
///
/// `rpc_result` – `Ok(height)` when the node replied, `Err(_)` when unreachable.
/// `last_status` – status from the previous iteration.
/// `last_height` – height from the previous iteration (`-1` when unknown).
pub fn step(
    rpc_result: Result<i64, String>,
    last_status: &Status,
    last_height: i64,
) -> Vec<Action> {
    let mut actions = Vec::new();

    match rpc_result {
        Err(_) => {
            if *last_status != Status::Down {
                actions.push(Action::Notify(
                    "Monad RPC is DOWN! Unable to connect to port.".to_string(),
                ));
                actions.push(Action::SetStatus(Status::Down));
            }
        }
        Ok(current_height) => {
            // Recovery from down
            if *last_status == Status::Down {
                actions.push(Action::Notify(format!(
                    "Monad RPC is back UP! Current height: {}",
                    current_height
                )));
                actions.push(Action::SetStatus(Status::Ok));
            }

            // Stuck detection
            if last_height == current_height && *last_status != Status::Stuck {
                actions.push(Action::Notify(format!(
                    "Monad node stuck at height: {}",
                    current_height
                )));
                actions.push(Action::SetStatus(Status::Stuck));
            } else if last_height != -1
                && last_height != current_height
                && *last_status == Status::Stuck
            {
                // Resumed after being stuck
                actions.push(Action::Notify(format!(
                    "Monad node syncing resumed! Current height: {}",
                    current_height
                )));
                actions.push(Action::SetStatus(Status::Ok));
            }

            actions.push(Action::SetHeight(current_height));

            // Keep status file fresh when advancing normally
            if last_height != current_height && *last_status != Status::Stuck {
                actions.push(Action::SetStatus(Status::Ok));
            }
        }
    }

    actions
}

// ---------------------------------------------------------------------------
// I/O helpers
// ---------------------------------------------------------------------------

/// Fetches the current block height from localhost:<RPC_PORT> using JSON-RPC.
/// Parses the hex block height (e.g. 0xbc23a5) into a decimal i64 (e.g. 12345669).
fn get_block_height(client: &Client) -> Result<i64, String> {
    let rpc_port = std::env::var("RPC_PORT").unwrap_or_else(|_| "8080".to_string());
    let url = format!("http://localhost:{}", rpc_port);

    let req_body = json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let resp = client
        .post(&url)
        .json(&req_body)
        .send()
        .map_err(|e| format!("RPC call failed: {}", e))?;

    let eth_resp: EthResponse = resp
        .json()
        .map_err(|e| format!("decode error: {}", e))?;

    let hex_str = eth_resp
        .result
        .strip_prefix("0x")
        .ok_or_else(|| "missing 0x prefix in result".to_string())?;

    i64::from_str_radix(hex_str, 16).map_err(|e| format!("hex to int error: {}", e))
}

/// Sends a message to the configured Telegram chat using the Bot API.
/// Reads TELEGRAM_TOKEN and TELEGRAM_CHAT_ID from the environment.
fn send_telegram_message(client: &Client, message: &str) -> Result<(), String> {
    let token = std::env::var("TELEGRAM_TOKEN").unwrap_or_default();
    let chat_id = std::env::var("TELEGRAM_CHAT_ID").unwrap_or_default();
    let url = format!("https://api.telegram.org/bot{}/sendMessage", token);

    let body = json!({
        "chat_id": chat_id,
        "text": message,
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .map_err(|e| format!("telegram API error: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("telegram returned non-OK status: {}", resp.status()));
    }
    Ok(())
}

fn read_int_from_file(path: &str) -> Option<i64> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn write_int_to_file(path: &str, val: i64) {
    let _ = fs::write(path, val.to_string());
}

fn read_string_from_file(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn write_string_to_file(path: &str, val: &str) {
    let _ = fs::write(path, val);
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main() {
    let _ = dotenvy::dotenv();
    let client = Client::new();

    loop {
        let last_status = Status::from_str(&read_string_from_file(LAST_STATUS_FILE));
        let last_height = read_int_from_file(LAST_HEIGHT_FILE).unwrap_or(-1);

        let rpc_result = get_block_height(&client);
        if let Err(ref e) = rpc_result {
            eprintln!("RPC error: {}", e);
        }

        for action in step(rpc_result, &last_status, last_height) {
            match action {
                Action::Notify(msg) => {
                    if let Err(e) = send_telegram_message(&client, &msg) {
                        eprintln!("Failed to send Telegram message: {}", e);
                    }
                }
                Action::SetStatus(s) => write_string_to_file(LAST_STATUS_FILE, s.as_str()),
                Action::SetHeight(h) => write_int_to_file(LAST_HEIGHT_FILE, h),
            }
        }

        thread::sleep(CHECK_INTERVAL);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Helper: collect only Notify messages from an action list
    // ------------------------------------------------------------------
    fn notifications(actions: &[Action]) -> Vec<&str> {
        actions
            .iter()
            .filter_map(|a| {
                if let Action::Notify(msg) = a {
                    Some(msg.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    // ------------------------------------------------------------------
    // Helper: find the final SetStatus in an action list (last one wins)
    // ------------------------------------------------------------------
    fn final_status(actions: &[Action]) -> Option<&Status> {
        actions
            .iter()
            .filter_map(|a| {
                if let Action::SetStatus(s) = a {
                    Some(s)
                } else {
                    None
                }
            })
            .last()
    }

    // ------------------------------------------------------------------
    // ok -> stuck
    // ------------------------------------------------------------------

    #[test]
    fn test_ok_to_stuck_when_height_unchanged() {
        // Height reported is the same as last time => stuck
        let actions = step(Ok(1000), &Status::Ok, 1000);
        assert!(
            notifications(&actions)
                .iter()
                .any(|m| m.contains("stuck at height: 1000")),
            "expected stuck notification, got: {:?}",
            actions
        );
        assert_eq!(final_status(&actions), Some(&Status::Stuck));
    }

    #[test]
    fn test_no_duplicate_stuck_alert_when_already_stuck() {
        // Already stuck and height still unchanged: no new alert
        let actions = step(Ok(1000), &Status::Stuck, 1000);
        assert!(
            notifications(&actions).is_empty(),
            "should not re-notify when already stuck, got: {:?}",
            actions
        );
    }

    #[test]
    fn test_ok_advancing_produces_no_alert() {
        // Height advances normally: no notification, status stays ok
        let actions = step(Ok(1001), &Status::Ok, 1000);
        assert!(
            notifications(&actions).is_empty(),
            "advancing height should not trigger any alert, got: {:?}",
            actions
        );
    }

    // ------------------------------------------------------------------
    // stuck -> down
    // ------------------------------------------------------------------

    #[test]
    fn test_stuck_to_down_on_rpc_failure() {
        // Node was stuck, now the RPC fails entirely
        let actions = step(Err("connection refused".to_string()), &Status::Stuck, 1000);
        assert!(
            notifications(&actions)
                .iter()
                .any(|m| m.contains("DOWN")),
            "expected DOWN notification, got: {:?}",
            actions
        );
        assert_eq!(final_status(&actions), Some(&Status::Down));
    }

    #[test]
    fn test_no_duplicate_down_alert_when_already_down() {
        // Already marked down: subsequent RPC failures must not re-alert
        let actions = step(Err("timeout".to_string()), &Status::Down, 1000);
        assert!(
            notifications(&actions).is_empty(),
            "should not re-notify when already down, got: {:?}",
            actions
        );
    }

    // ------------------------------------------------------------------
    // down -> ok (recovery)
    // ------------------------------------------------------------------

    #[test]
    fn test_down_to_ok_recovery() {
        // Node was down, RPC now returns a valid height
        let actions = step(Ok(2000), &Status::Down, 1999);
        assert!(
            notifications(&actions)
                .iter()
                .any(|m| m.contains("back UP") && m.contains("2000")),
            "expected UP recovery notification, got: {:?}",
            actions
        );
        assert_eq!(final_status(&actions), Some(&Status::Ok));
    }

    #[test]
    fn test_down_to_ok_persists_height() {
        // Recovery must also record the new height
        let actions = step(Ok(2000), &Status::Down, 1999);
        assert!(
            actions.contains(&Action::SetHeight(2000)),
            "expected SetHeight(2000) action, got: {:?}",
            actions
        );
    }

    // ------------------------------------------------------------------
    // stuck -> ok (resumed syncing)
    // ------------------------------------------------------------------

    #[test]
    fn test_stuck_to_ok_when_height_advances() {
        // Was stuck, height now advanced => resumed notification
        let actions = step(Ok(1001), &Status::Stuck, 1000);
        assert!(
            notifications(&actions)
                .iter()
                .any(|m| m.contains("syncing resumed") && m.contains("1001")),
            "expected 'syncing resumed' notification, got: {:?}",
            actions
        );
        assert_eq!(final_status(&actions), Some(&Status::Ok));
    }

    // ------------------------------------------------------------------
    // Edge cases
    // ------------------------------------------------------------------

    #[test]
    fn test_first_run_no_alert_when_last_height_unknown() {
        // On the very first run last_height is -1; same current height
        // should not falsely trigger stuck (guard: last_height != -1).
        // Actually the stuck check uses last_height == current_height,
        // so height -1 will never equal a real block height — but let's
        // also verify that a fresh start with a real height doesn't alert.
        let actions = step(Ok(500), &Status::Ok, -1);
        assert!(
            notifications(&actions).is_empty(),
            "first run with unknown previous height should not alert, got: {:?}",
            actions
        );
    }

    #[test]
    fn test_ok_to_down_directly() {
        // Node was ok, suddenly unreachable
        let actions = step(Err("refused".to_string()), &Status::Ok, 800);
        assert!(
            notifications(&actions)
                .iter()
                .any(|m| m.contains("DOWN")),
            "expected DOWN notification, got: {:?}",
            actions
        );
        assert_eq!(final_status(&actions), Some(&Status::Down));
    }

    // ------------------------------------------------------------------
    // Status round-trip
    // ------------------------------------------------------------------

    #[test]
    fn test_status_from_str_round_trip() {
        for s in &["ok", "stuck", "down"] {
            assert_eq!(Status::from_str(s).as_str(), *s);
        }
    }

    #[test]
    fn test_status_from_str_unknown_defaults_to_ok() {
        assert_eq!(Status::from_str(""), Status::Ok);
        assert_eq!(Status::from_str("unknown"), Status::Ok);
    }
}
