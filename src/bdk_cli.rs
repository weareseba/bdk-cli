// Magical Bitcoin Library
// Written in 2020 by
//     Alekos Filini <alekos.filini@gmail.com>
//
// Copyright (c) 2020 Magical Bitcoin
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use std::fs;
use std::path::PathBuf;

#[cfg(feature = "rpc")]
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use clap::AppSettings;
use log::{debug, error, info, warn};

#[cfg(feature = "repl")]
use rustyline::error::ReadlineError;
#[cfg(feature = "repl")]
use rustyline::Editor;

use structopt::StructOpt;

#[cfg(feature = "compact_filters")]
use bdk::blockchain::compact_filters::{BitcoinPeerConfig, CompactFiltersBlockchainConfig};
#[cfg(feature = "electrum")]
use bdk::blockchain::electrum::ElectrumBlockchainConfig;
#[cfg(feature = "esplora")]
use bdk::blockchain::esplora::EsploraBlockchainConfig;

#[cfg(any(
    feature = "electrum",
    feature = "esplora",
    feature = "compact_filters",
    feature = "rpc"
))]
use bdk::blockchain::{AnyBlockchain, AnyBlockchainConfig, ConfigurableBlockchain};

#[cfg(feature = "rpc")]
use bdk::blockchain::rpc::{wallet_name_from_descriptor, Auth, RpcConfig};

use bdk::database::BatchDatabase;
use bdk::database::MemoryDatabase;
use bdk::Wallet;
use bdk::{bitcoin, Error};
use bdk_cli::WalletSubCommand;
use bdk_cli::{CliOpts, CliSubCommand, KeySubCommand, OfflineWalletSubCommand, WalletOpts};

#[cfg(any(
    feature = "electrum",
    feature = "esplora",
    feature = "compact_filters",
    feature = "rpc"
))]
use bdk_cli::OnlineWalletSubCommand;

#[cfg(feature = "repl")]
use regex::Regex;

#[cfg(feature = "repl")]
const REPL_LINE_SPLIT_REGEX: &str = r#""([^"]*)"|'([^']*)'|([\w\-]+)"#;

/// REPL mode
#[derive(Debug, StructOpt, Clone, PartialEq)]
#[structopt(name = "", setting = AppSettings::NoBinaryName,
version = option_env ! ("CARGO_PKG_VERSION").unwrap_or("unknown"),
author = option_env ! ("CARGO_PKG_AUTHORS").unwrap_or(""))]
pub enum ReplSubCommand {
    #[cfg(any(
        feature = "electrum",
        feature = "esplora",
        feature = "compact_filters",
        feature = "rpc"
    ))]
    #[structopt(flatten)]
    OnlineWalletSubCommand(OnlineWalletSubCommand),
    #[structopt(flatten)]
    OfflineWalletSubCommand(OfflineWalletSubCommand),
    #[structopt(flatten)]
    KeySubCommand(KeySubCommand),
    /// Exit REPL loop
    Exit,
}

fn prepare_home_dir() -> PathBuf {
    let mut dir = PathBuf::new();
    dir.push(&dirs_next::home_dir().unwrap());
    dir.push(".bdk-bitcoin");

    if !dir.exists() {
        info!("Creating home directory {}", dir.as_path().display());
        fs::create_dir(&dir).unwrap();
    }

    #[cfg(not(feature = "compact_filters"))]
    dir.push("database.sled");

    #[cfg(feature = "compact_filters")]
    dir.push("compact_filters");
    dir
}

#[cfg(any(
    feature = "electrum",
    feature = "esplora",
    feature = "compact_filters",
    feature = "rpc"
))]
fn new_online_wallet<D>(
    network: Network,
    wallet_opts: &WalletOpts,
    database: D,
) -> Result<Wallet<AnyBlockchain, D>, Error>
where
    D: BatchDatabase,
{
    #[cfg(feature = "electrum")]
    let config = AnyBlockchainConfig::Electrum(ElectrumBlockchainConfig {
        url: wallet_opts.electrum_opts.server.clone(),
        socks5: wallet_opts.proxy_opts.proxy.clone(),
        retry: wallet_opts.proxy_opts.retries,
        timeout: wallet_opts.electrum_opts.timeout,
        stop_gap: wallet_opts.electrum_opts.stop_gap,
    });

    #[cfg(feature = "esplora-ureq")]
    let config = AnyBlockchainConfig::Esplora(EsploraBlockchainConfig {
        base_url: wallet_opts.esplora_opts.server.clone(),
        timeout_read: wallet_opts.esplora_opts.read_timeout,
        timeout_write: wallet_opts.esplora_opts.write_timeout,
        stop_gap: wallet_opts.esplora_opts.stop_gap,
        proxy: wallet_opts.proxy_opts.proxy.clone(),
    });

    #[cfg(feature = "esplora-reqwest")]
    let config = AnyBlockchainConfig::Esplora(EsploraBlockchainConfig {
        base_url: wallet_opts.esplora_opts.server.clone(),
        concurrency: Some(wallet_opts.esplora_opts.conc),
        stop_gap: wallet_opts.esplora_opts.stop_gap,
        proxy: wallet_opts.proxy_opts.proxy.clone(),
    });

    #[cfg(feature = "compact_filters")]
    let config = {
        let mut peers = vec![];
        for addrs in wallet_opts.compactfilter_opts.address.clone() {
            for _ in 0..wallet_opts.compactfilter_opts.conn_count {
                peers.push(BitcoinPeerConfig {
                    address: addrs.clone(),
                    socks5: wallet_opts.proxy_opts.proxy.clone(),
                    socks5_credentials: wallet_opts.proxy_opts.proxy_auth.clone(),
                })
            }
        }

        AnyBlockchainConfig::CompactFilters(CompactFiltersBlockchainConfig {
            peers,
            network,
            storage_dir: prepare_home_dir().into_os_string().into_string().unwrap(),
            skip_blocks: Some(wallet_opts.compactfilter_opts.skip_blocks),
        })
    };

    #[cfg(feature = "rpc")]
    let config: AnyBlockchainConfig = {
        let auth = Auth::UserPass {
            username: wallet_opts.rpc_opts.auth.0.clone(),
            password: wallet_opts.rpc_opts.auth.1.clone(),
        };

        // Use deterministic wallet name derived from descriptor
        let wallet_name = wallet_name_from_descriptor(
            &wallet_opts.descriptor[..],
            wallet_opts.change_descriptor.as_deref(),
            network,
            &Secp256k1::new(),
        )?;

        let mut rpc_url = "http://".to_string();
        rpc_url.push_str(&wallet_opts.rpc_opts.address[..]);

        let rpc_config = RpcConfig {
            url: rpc_url,
            auth,
            network,
            wallet_name,
            skip_blocks: wallet_opts.rpc_opts.skip_blocks,
        };

        AnyBlockchainConfig::Rpc(rpc_config)
    };
    let descriptor = wallet_opts.descriptor.as_str();
    let change_descriptor = wallet_opts.change_descriptor.as_deref();

    let wallet = Wallet::new(
        descriptor,
        change_descriptor,
        network,
        database,
        AnyBlockchain::from_config(&config)?,
    )?;
    Ok(wallet)
}

fn new_offline_wallet<D>(
    network: Network,
    wallet_opts: &WalletOpts,
    database: D,
) -> Result<Wallet<(), D>, Error>
where
    D: BatchDatabase,
{
    let descriptor = wallet_opts.descriptor.as_str();
    let change_descriptor = wallet_opts.change_descriptor.as_deref();
    let wallet = Wallet::new_offline(descriptor, change_descriptor, network, database)?;
    Ok(wallet)
}

fn main() {
    env_logger::init();

    let cli_opts: CliOpts = CliOpts::from_args();

    let network = cli_opts.network;
    debug!("network: {:?}", network);
    if network == Network::Bitcoin {
        warn!("This is experimental software and not currently recommended for use on Bitcoin mainnet, proceed with caution.")
    }

    match handle_command(cli_opts, network) {
        Ok(result) => println!("{}", result),
        Err(e) => {
            match e {
                Error::ChecksumMismatch => error!("Descriptor checksum mismatch. Are you using a different descriptor for an already defined wallet name? (if you are not specifying the wallet name it defaults to 'main')"),
                e => error!("{}", e.to_string()),
            }
        },
    }
}

fn handle_command(cli_opts: CliOpts, network: Network) -> Result<String, Error> {
    let result = match cli_opts.subcommand {
        #[cfg(any(
            feature = "electrum",
            feature = "esplora",
            feature = "compact_filters",
            feature = "rpc"
        ))]
        CliSubCommand::Wallet {
            wallet_opts,
            subcommand: WalletSubCommand::OnlineWalletSubCommand(online_subcommand),
        } => {
            let database = MemoryDatabase::default();
            let wallet = new_online_wallet(network, &wallet_opts, database)?;
            let result = bdk_cli::handle_online_wallet_subcommand(&wallet, online_subcommand)?;
            serde_json::to_string_pretty(&result)?
        }
        CliSubCommand::Wallet {
            wallet_opts,
            subcommand: WalletSubCommand::OfflineWalletSubCommand(offline_subcommand),
        } => {
            let database = MemoryDatabase::default();
            let wallet = new_offline_wallet(network, &wallet_opts, database)?;
            let result = bdk_cli::handle_offline_wallet_subcommand(
                &wallet,
                &wallet_opts,
                offline_subcommand,
            )?;
            serde_json::to_string_pretty(&result)?
        }
        CliSubCommand::Key {
            subcommand: key_subcommand,
        } => {
            let result = bdk_cli::handle_key_subcommand(network, key_subcommand)?;
            serde_json::to_string_pretty(&result)?
        }
        #[cfg(feature = "compiler")]
        CliSubCommand::Compile {
            policy,
            script_type,
        } => {
            let result = bdk_cli::handle_compile_subcommand(network, policy, script_type)?;
            serde_json::to_string_pretty(&result)?
        }
        #[cfg(feature = "repl")]
        CliSubCommand::Repl { wallet_opts } => {
            let database = MemoryDatabase::default();

            #[cfg(any(
                feature = "electrum",
                feature = "esplora",
                feature = "compact_filters",
                feature = "rpc"
            ))]
            let wallet = new_online_wallet(network, &wallet_opts, database)?;

            #[cfg(not(any(
                feature = "electrum",
                feature = "esplora",
                feature = "compact_filters",
                feature = "rpc"
            )))]
            let wallet = new_offline_wallet(network, &wallet_opts, database)?;

            let mut rl = Editor::<()>::new();

            // if rl.load_history("history.txt").is_err() {
            //     println!("No previous history.");
            // }

            let split_regex =
                Regex::new(REPL_LINE_SPLIT_REGEX).map_err(|e| Error::Generic(e.to_string()))?;

            loop {
                let readline = rl.readline(">> ");
                match readline {
                    Ok(line) => {
                        if line.trim() == "" {
                            continue;
                        }
                        rl.add_history_entry(line.as_str());
                        let split_line: Vec<&str> = split_regex
                            .captures_iter(&line)
                            .map(|c| {
                                c.get(1)
                                    .or_else(|| c.get(2))
                                    .or_else(|| c.get(3))
                                    .unwrap()
                                    .as_str()
                            })
                            .collect();
                        let repl_subcommand: Result<ReplSubCommand, clap::Error> =
                            ReplSubCommand::from_iter_safe(split_line);
                        debug!("repl_subcommand = {:?}", repl_subcommand);

                        if let Err(err) = repl_subcommand {
                            println!("{}", err.message);
                            continue;
                        }

                        let repl_subcommand = repl_subcommand.unwrap();

                        let result = match repl_subcommand {
                            #[cfg(any(
                                feature = "electrum",
                                feature = "esplora",
                                feature = "compact_filters",
                                feature = "rpc"
                            ))]
                            ReplSubCommand::OnlineWalletSubCommand(online_subcommand) => {
                                bdk_cli::handle_online_wallet_subcommand(&wallet, online_subcommand)
                            }
                            ReplSubCommand::OfflineWalletSubCommand(offline_subcommand) => {
                                bdk_cli::handle_offline_wallet_subcommand(
                                    &wallet,
                                    &wallet_opts,
                                    offline_subcommand,
                                )
                            }
                            ReplSubCommand::KeySubCommand(key_subcommand) => {
                                bdk_cli::handle_key_subcommand(network, key_subcommand)
                            }
                            ReplSubCommand::Exit => break,
                        };

                        println!(
                            "{}",
                            serde_json::to_string_pretty(&result.unwrap()).unwrap()
                        );
                    }
                    Err(ReadlineError::Interrupted) => continue,
                    Err(ReadlineError::Eof) => break,
                    Err(err) => {
                        println!("{:?}", err);
                        break;
                    }
                }
            }

            // rl.save_history("history.txt").unwrap();
            "Exiting REPL".to_string()
        }
    };
    Ok(result)
}

#[cfg(test)]
mod test {
    use crate::REPL_LINE_SPLIT_REGEX;
    use regex::Regex;

    #[test]
    fn test_regex_double_quotes() {
        let split_regex = Regex::new(REPL_LINE_SPLIT_REGEX).unwrap();
        let line = r#"restore -m "word1 word2 word3" -p 'test! 123 -test' "#;
        let split_line: Vec<&str> = split_regex
            .captures_iter(&line)
            .map(|c| {
                c.get(1)
                    .or_else(|| c.get(2))
                    .or_else(|| c.get(3))
                    .unwrap()
                    .as_str()
            })
            .collect();
        assert_eq!(
            vec!(
                "restore",
                "-m",
                "word1 word2 word3",
                "-p",
                "test! 123 -test"
            ),
            split_line
        );
    }

    #[test]
    fn test_regex_single_quotes() {
        let split_regex = Regex::new(REPL_LINE_SPLIT_REGEX).unwrap();
        let line = r#"restore -m 'word1 word2 word3' -p "test *123 -test" "#;
        let split_line: Vec<&str> = split_regex
            .captures_iter(&line)
            .map(|c| {
                c.get(1)
                    .or_else(|| c.get(2))
                    .or_else(|| c.get(3))
                    .unwrap()
                    .as_str()
            })
            .collect();
        assert_eq!(
            vec!(
                "restore",
                "-m",
                "word1 word2 word3",
                "-p",
                "test *123 -test"
            ),
            split_line
        );
    }
}
