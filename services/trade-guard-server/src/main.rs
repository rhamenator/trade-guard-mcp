//! `trade-guard-server` — the `trade-guard-mcp` binary. Paper-only
//! vertical slice: `mcp` runs the stdio JSON-RPC server;
//! `self-test` runs the same internal invariant checks the `self-test`
//! MCP tool exposes and reports a process exit code, matching
//! `smart-dynamic-hedge`'s own CLI convention for a scriptable pass/fail
//! signal.

use std::path::PathBuf;

use trade_guard_core::account::AccountSnapshot;
use trade_guard_core::decimal::Decimal;
use trade_guard_core::mcp::run_stdio;
use trade_guard_core::state::GuardState;
use trade_guard_core::tools::self_test;
use trade_guard_core::utc_timestamp::UtcTimestamp;

const DEFAULT_ACCOUNT_ALIAS: &str = "paper-default";
const DEFAULT_STARTING_CASH: i64 = 100_000;

fn audit_db_path() -> PathBuf {
    std::env::var("TRADE_GUARD_DB").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(".trade-guard").join("audit.sqlite3"))
}

fn build_state() -> Result<GuardState, String> {
    let account = AccountSnapshot::new(DEFAULT_ACCOUNT_ALIAS, "USD", Decimal::from_i64(DEFAULT_STARTING_CASH), UtcTimestamp::now());
    GuardState::new(account, audit_db_path()).map_err(|e| e.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(String::as_str).unwrap_or("mcp");

    match command {
        "mcp" => {
            let mut state = match build_state() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("trade-guard-server: failed to initialize: {e}");
                    std::process::exit(1);
                }
            };
            if let Err(e) = run_stdio(&mut state) {
                eprintln!("trade-guard-server: stdio transport error: {e}");
                std::process::exit(1);
            }
        }
        "self-test" => {
            let state = match build_state() {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("trade-guard-server: failed to initialize: {e}");
                    std::process::exit(1);
                }
            };
            match self_test(&state) {
                Ok(report) => {
                    println!("{report}");
                    println!("self-test: PASS");
                }
                Err(report) => {
                    println!("{report}");
                    println!("self-test: FAIL");
                    std::process::exit(1);
                }
            }
        }
        other => {
            eprintln!("trade-guard-server: unknown command {other:?} (expected \"mcp\" or \"self-test\")");
            std::process::exit(2);
        }
    }
}
