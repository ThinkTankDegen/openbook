use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use solana_cli_output::display::println_transaction;
use tokio::time::{sleep, Duration};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

// Re-exports from the openbook crate
use openbook::commitment_config::CommitmentConfig;
use openbook::matching::Side;
use openbook::v1::ob_client::{OBClient, PROGRAM_ID_ENV, SRM_PROGRAM_ID};
use openbook::v1::orders::OrderReturnType;

use openbook::pubkey::Pubkey;
use openbook::signature::Signature;

use std::str::FromStr;

const CRANK_DELAY_MS: u64 = 50_000;
const MAX_CANCEL_ORDERS: usize = 5;
const MAX_CANCEL_ORDERS_PER_TX: usize = 5;

/// Simple v1-only CLI for OpenBook.
#[derive(Parser, Debug)]
#[command(
    author = "You",
    version,
    about = "OpenBook v1 CLI (no v2, no TUI)",
    long_about = None
)]
struct Cli {
    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Market id to trade on (OpenBook v1 market address)
    #[arg(
        short,
        long,
        default_value_t = String::from(
            "8BnEgHoWFysVcuFFX7QztDmzuH8r5ZFvyP3sYwn1XTh6"
        )
    )]
    market_id: String,

    /// Program id of the DEX (OpenBook v1 by default, Serum v3 supported)
    #[arg(
        long,
        default_value = SRM_PROGRAM_ID,
        help = "DEX program id to target (e.g. 9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin for Serum v3)"
    )]
    program_id: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Fetch market info & current open orders
    Info,

    /// Display event queue status (pending events, head, seq)
    EventQueue,

    /// Place a limit order (bid / ask)
    Place(Place),

    /// Cancel all open orders for your OOS account
    Cancel(Cancel),

    /// Settle balances
    Settle(Settle),

    /// Match orders (crank)
    Match(MatchOrders),

    /// Cancel, settle, place both bid & ask
    CancelSettlePlace(CancelSettlePlace),

    /// Cancel, settle, place only bid
    CancelSettlePlaceBid(CancelSettlePlaceBid),

    /// Cancel, settle, place only ask
    CancelSettlePlaceAsk(CancelSettlePlaceAsk),

    /// Consume events
    Consume(Consume),

    /// Consume events (permissioned)
    ConsumePermissioned(ConsumePermissioned),

    /// Load orders for owner
    LoadOrders,

    /// Find open orders accounts for owner
    FindOpenOrders,
}

// Argument structs mirror `src/cli.rs` from the original repo.

#[derive(Args, Debug, Clone)]
struct Place {
    /// Target amount in quote currency (e.g. USDC)
    #[arg(short, long)]
    target_amount_quote: f64,

    /// Side: "bid" or "ask"
    #[arg(short, long)]
    side: String,

    /// Best offset in USDC
    #[arg(short, long)]
    best_offset_usdc: f64,

    /// Execute on-chain (if false, only build instructions)
    #[arg(short, long)]
    execute: bool,

    /// Target price
    #[arg(short, long)]
    price_target: f64,
}

#[derive(Args, Debug, Clone)]
struct Cancel {
    /// Execute on-chain (if false, only build instructions)
    #[arg(short, long)]
    execute: bool,
}

#[derive(Args, Debug, Clone)]
struct Settle {
    /// Execute on-chain (if false, only build instructions)
    #[arg(short, long)]
    execute: bool,
}

#[derive(Args, Debug, Clone)]
struct MatchOrders {
    /// Maximum number of orders to match
    #[arg(short, long)]
    limit: u16,
}

#[derive(Args, Debug, Clone)]
struct CancelSettlePlace {
    /// Target size in USDC for the ask order
    #[arg(short, long)]
    usdc_ask_target: f64,

    /// Target size in USDC for the bid order
    #[arg(short, long)]
    target_usdc_bid: f64,

    /// Bid price in JLP/USDC
    #[arg(short, long)]
    price_jlp_usdc_bid: f64,

    /// Ask price in JLP/USDC
    #[arg(short, long)]
    ask_price_jlp_usdc: f64,
}

#[derive(Args, Debug, Clone)]
struct CancelSettlePlaceBid {
    /// Target size in USDC for the bid order
    #[arg(short, long)]
    target_size_usdc_bid: f64,

    /// Bid price in JLP/USDC
    #[arg(short, long)]
    bid_price_jlp_usdc: f64,
}

#[derive(Args, Debug, Clone)]
struct CancelSettlePlaceAsk {
    /// Target size in USDC for the ask order
    #[arg(short, long)]
    target_size_usdc_ask: f64,

    /// Ask price in JLP/USDC
    #[arg(short, long)]
    ask_price_jlp_usdc: f64,
}

#[derive(Args, Debug, Clone)]
struct Consume {
    /// Limit for consume events instruction
    #[arg(short, long)]
    limit: u16,

    /// Open orders accounts to crank (comma separated or repeated flag). Defaults to your --oos key
    #[arg(
        long = "open-orders",
        value_name = "PUBKEY",
        value_delimiter = ',',
        num_args = 1..
    )]
    open_orders: Vec<String>,
}

#[derive(Args, Debug, Clone)]
struct ConsumePermissioned {
    /// Limit for consume events permissioned instruction
    #[arg(short, long)]
    limit: u16,

    /// Open orders accounts to crank (comma separated or repeated flag). Defaults to your --oos key
    #[arg(
        long = "open-orders",
        value_name = "PUBKEY",
        value_delimiter = ',',
        num_args = 1..
    )]
    open_orders: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Basic logging
    let filter = match std::env::var("RUST_LOG") {
        Ok(val) => EnvFilter::new(val),
        Err(_) => EnvFilter::new("info"),
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .with_line_number(false)
        .compact()
        .init();

    let cli = Cli::parse();

    // Configure the target program id before instantiating the client
    std::env::set_var(PROGRAM_ID_ENV, &cli.program_id);

    // Instantiate OB v1 client
    let market_id = cli.market_id.parse()?;
    let mut ob_client = OBClient::new(
        CommitmentConfig::confirmed(),
        market_id,
        true,          // use_cache
        123456789_u128 // cache_ts (just a nonce)
    )
    .await?;

    match cli.command {
        Commands::Info => {
            info!("[*] OB_V1_Client:\n{:#?}", ob_client);
        }

        Commands::EventQueue => {
            let stats = ob_client.fetch_event_queue_stats().await?;
            let needs_crank = stats.count > 0;
            info!(
                "[*] Event queue stats => pending_events: {}, head: {}, seq_num: {}, account_flags: 0x{:x}, needs_crank: {}",
                stats.count, stats.head, stats.seq_num, stats.account_flags, needs_crank
            );
        }

        Commands::Place(arg) => {
            let side = match arg.side.to_ascii_lowercase().as_str() {
                "bid" => Side::Bid,
                "ask" => Side::Ask,
                _ => Side::Bid,
            };

            if let Some(ord_ret_type) = ob_client
                .place_limit_order(
                    arg.target_amount_quote,
                    side,
                    arg.best_offset_usdc,
                    arg.execute,
                    arg.price_target,
                )
                .await?
            {
                handle_order_return(&mut ob_client, ord_ret_type).await?;
            }
        }

        Commands::Cancel(arg) => {
            if arg.execute {
                if let Some(signature) = execute_limited_cancel(
                    &mut ob_client,
                    MAX_CANCEL_ORDERS,
                    MAX_CANCEL_ORDERS_PER_TX,
                )
                .await?
                {
                    info!("\n[*] Transaction successful, signature: {:?}", signature);
                    show_tx(&mut ob_client, &signature).await?;
                }
            } else if let Some(ord_ret_type) = ob_client.cancel_orders(false).await? {
                handle_order_return(&mut ob_client, ord_ret_type).await?;
            }
        }

        Commands::Settle(arg) => {
            if let Some(ord_ret_type) = ob_client.settle_balance(arg.execute).await? {
                handle_order_return(&mut ob_client, ord_ret_type).await?;
            }
        }

        Commands::Match(arg) => {
            let (_confirmed, signature) =
                ob_client.match_orders_transaction(arg.limit).await?;
            info!("\n[*] Transaction successful, signature: {:?}", signature);
            show_tx(&mut ob_client, &signature).await?;
        }

        Commands::CancelSettlePlace(arg) => {
            let (_confirmed, signature) = ob_client
                .cancel_settle_place(
                    arg.usdc_ask_target,
                    arg.target_usdc_bid,
                    arg.price_jlp_usdc_bid,
                    arg.ask_price_jlp_usdc,
                )
                .await?;
            info!("\n[*] Transaction successful, signature: {:?}", signature);
            show_tx(&mut ob_client, &signature).await?;
        }

        Commands::CancelSettlePlaceBid(arg) => {
            let (_confirmed, signature) = ob_client
                .cancel_settle_place_bid(
                    arg.target_size_usdc_bid,
                    arg.bid_price_jlp_usdc,
                )
                .await?;
            info!("\n[*] Transaction successful, signature: {:?}", signature);
            show_tx(&mut ob_client, &signature).await?;
        }

        Commands::CancelSettlePlaceAsk(arg) => {
            let (_confirmed, signature) = ob_client
                .cancel_settle_place_ask(
                    arg.target_size_usdc_ask,
                    arg.ask_price_jlp_usdc,
                )
                .await?;
            info!("\n[*] Transaction successful, signature: {:?}", signature);
            show_tx(&mut ob_client, &signature).await?;
        }

        Commands::Consume(arg) => {
            let open_orders = if arg.open_orders.is_empty() {
                let mut owners = ob_client
                    .collect_event_queue_open_orders(arg.limit as usize)
                    .await?;
                if owners.is_empty() {
                    owners.push(ob_client.open_orders.oo_key);
                }
                owners
            } else {
                parse_open_orders(&arg.open_orders)?
            };
            let (_confirmed, signature) = ob_client
                .consume_events_instruction(open_orders, arg.limit)
                .await?;
            info!("\n[*] Transaction successful, signature: {:?}", signature);
            show_tx(&mut ob_client, &signature).await?;
        }

        Commands::ConsumePermissioned(arg) => {
            let open_orders = if arg.open_orders.is_empty() {
                let mut owners = ob_client
                    .collect_event_queue_open_orders(arg.limit as usize)
                    .await?;
                if owners.is_empty() {
                    owners.push(ob_client.open_orders.oo_key);
                }
                owners
            } else {
                parse_open_orders(&arg.open_orders)?
            };
            let (_confirmed, signature) = ob_client
                .consume_events_permissioned_instruction(open_orders, arg.limit)
                .await?;
            info!("\n[*] Transaction successful, signature: {:?}", signature);
            show_tx(&mut ob_client, &signature).await?;
        }

        Commands::LoadOrders => {
            match ob_client.load_orders_for_owner().await {
                Ok(l) => {
                    info!("\n[*] Found Program Accounts: {:#?}", l);
                }
                Err(e) => {
                    eprintln!("[*] Error loading orders for owner: {e}");
                }
            }
        }

        Commands::FindOpenOrders => {
            match ob_client
                .find_open_orders_accounts_for_owner(ob_client.open_orders.oo_key, 1000)
                .await
            {
                Ok(result) => {
                    info!("\n[*] Found Open Orders Accounts: {:#?}", result);
                }
                Err(e) => {
                    eprintln!("[*] Error finding open orders accounts: {e}");
                }
            }
        }
    }

    Ok(())
}

async fn handle_order_return(
    ob_client: &mut OBClient,
    ord_ret_type: OrderReturnType,
) -> Result<()> {
    match ord_ret_type {
        OrderReturnType::Instructions(insts) => {
            info!("\n[*] Got Instructions: {:?}", insts);
        }
        OrderReturnType::Signature(signature) => {
            info!("\n[*] Transaction successful, signature: {:?}", signature);
            show_tx(ob_client, &signature).await?;
        }
    }
    Ok(())
}

async fn show_tx(ob_client: &mut OBClient, signature: &Signature) -> Result<()> {
    // wait for crank
    sleep(Duration::from_millis(CRANK_DELAY_MS)).await;

    match ob_client.rpc_client.fetch_transaction(signature).await {
        Ok(confirmed_tx) => {
            println_transaction(
                &confirmed_tx
                    .transaction
                    .transaction
                    .decode()
                    .expect("Successful decode"),
                confirmed_tx.transaction.meta.as_ref(),
                " ",
                None,
                None,
            );
        }
        Err(err) => {
            error!(
                "[*] Unable to get confirmed transaction details: {}",
                err
            );
        }
    }

    Ok(())
}

async fn execute_limited_cancel(
    ob_client: &mut OBClient,
    max_instructions: usize,
    max_instructions_per_tx: usize,
) -> Result<Option<Signature>> {
    let ord_ret_type = match ob_client.cancel_orders(false).await? {
        Some(ret) => ret,
        None => return Ok(None),
    };

    let OrderReturnType::Instructions(mut insts) = ord_ret_type else {
        return Ok(None);
    };

    if insts.is_empty() {
        return Ok(None);
    }

    insts.truncate(max_instructions);

    let mut last_sig = None;
    for chunk in insts.chunks(max_instructions_per_tx.max(1)) {
        let (_, signature) = ob_client
            .rpc_client
            .send_and_confirm((*ob_client.owner).insecure_clone(), chunk.to_vec())
            .await?;
        last_sig = Some(signature);
    }

    Ok(last_sig)
}

fn parse_open_orders(inputs: &[String]) -> Result<Vec<Pubkey>> {
    let mut keys = Vec::new();
    for key in inputs {
        let parsed = Pubkey::from_str(key)
            .map_err(|e| anyhow!("Invalid open orders pubkey '{key}': {e}"))?;
        keys.push(parsed);
    }
    Ok(keys)
}

