//! Native V3 `transfer-store` command — decentralized IPFS pattern store.
//!
//! Source: `v3/@claude-flow/cli/src/commands/transfer-store.ts`. Subcommands:
//! list/search/download/publish/info. IPFS registry not available in native
//! build; degrades with documented messages.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferStoreCommand {
    pub operation: String,
    pub query: Option<String>,
    pub registry: Option<String>,
    pub category: Option<String>,
    pub featured: bool,
    pub trending: bool,
    pub newest: bool,
    pub limit: usize,
    pub id: Option<String>,
    pub file: Option<String>,
    pub name: Option<String>,
    pub cid: Option<String>,
}

pub fn run(_root: &Path, command: TransferStoreCommand) -> u8 {
    match command.operation.as_str() {
        "" => {
            println!("\nRuFlo Pattern Store");
            println!("Decentralized pattern marketplace via IPFS");
            println!();
            println!("Subcommands:");
            println!("  list      List available patterns");
            println!("  search    Search patterns");
            println!("  download  Download a pattern by CID");
            println!("  publish   Publish a pattern");
            println!("  info      Show pattern details");
            println!();
            println!("Example:");
            println!("  claude-flow transfer-store list --featured");
            0
        }
        "list" => {
            eprintln!("[ERROR] IPFS pattern store not available in native build.");
            eprintln!("  The pattern store requires an IPFS registry client.");
            eprintln!("  Use: npx ruflo transfer-store list");
            1
        }
        "search" => {
            let query = command.query.as_deref().unwrap_or("");
            eprintln!("[ERROR] IPFS pattern search not available in native build.");
            eprintln!("  Query was: \"{query}\"");
            eprintln!("  Use: npx ruflo transfer-store search -q \"{query}\"");
            1
        }
        "download" => {
            let cid = command
                .cid
                .as_deref()
                .or(command.id.as_deref())
                .unwrap_or("");
            if cid.is_empty() {
                eprintln!("[ERROR] --cid is required");
                return 1;
            }
            eprintln!("[ERROR] IPFS download not available in native build.");
            eprintln!("  CID: {cid}");
            eprintln!("  Use: npx ruflo transfer-store download --cid {cid}");
            1
        }
        "publish" => {
            let file = command.file.as_deref().unwrap_or("");
            if file.is_empty() {
                eprintln!("[ERROR] --file is required");
                return 1;
            }
            if !Path::new(file).exists() {
                eprintln!("[ERROR] File not found: {file}");
                return 1;
            }
            eprintln!("[ERROR] IPFS publish not available in native build.");
            eprintln!("  Use: npx ruflo transfer-store publish -f {file}");
            1
        }
        "info" => {
            let id = command.id.as_deref().unwrap_or("");
            if id.is_empty() {
                eprintln!("[ERROR] --id is required");
                return 1;
            }
            eprintln!("[ERROR] IPFS pattern info not available in native build.");
            eprintln!("  ID: {id}");
            eprintln!("  Use: npx ruflo transfer-store info --id {id}");
            1
        }
        _ => {
            eprintln!(
                "[ERROR] Unknown: {} (list|search|download|publish|info)",
                command.operation
            );
            1
        }
    }
}
