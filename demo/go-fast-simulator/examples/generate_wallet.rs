//! Generate a new wallet and derive addresses for different Cosmos chains
//!
//! Usage: cargo run --example generate_wallet [output_file]

use k256::ecdsa::SigningKey;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    let output_file = args.get(1).map(|s| s.as_str()).unwrap_or(".env.local");

    println!("==========================================");
    println!("  COSMOS WALLET GENERATOR");
    println!("==========================================\n");

    // Generate a new random key
    let signing_key = SigningKey::random(&mut rand::thread_rng());
    let private_key_bytes = signing_key.to_bytes();
    let private_key_hex = hex::encode(&private_key_bytes);

    println!("Private Key (hex):");
    println!("  {}\n", private_key_hex);

    // Derive public key
    let verifying_key = signing_key.verifying_key();
    let pubkey_bytes = verifying_key.to_sec1_bytes();

    println!("Public Key (compressed, hex):");
    println!("  {}\n", hex::encode(&pubkey_bytes));

    // SHA256 -> RIPEMD160 for address
    let sha256_hash = Sha256::digest(&pubkey_bytes);
    let ripemd_hash = Ripemd160::digest(&sha256_hash);

    println!("Addresses:");

    let chains = vec![
        ("cosmos", "Cosmos Hub", "theta-testnet-001"),
        ("osmo", "Osmosis", "osmo-test-5"),
        ("neutron", "Neutron", "pion-1"),
        ("celestia", "Celestia", "mocha-4"),
    ];

    for (prefix, name, testnet) in &chains {
        let hrp = bech32::Hrp::parse(prefix).expect("Invalid prefix");
        let address = bech32::encode::<bech32::Bech32>(hrp, ripemd_hash.as_slice())
            .expect("Bech32 encoding failed");
        println!("  {} ({}):", name, testnet);
        println!("    {}", address);
    }

    // Save to files
    println!("\n==========================================");
    println!("  SAVING FILES");
    println!("==========================================\n");

    // Save private key to file
    let key_file = "wallet.key";
    fs::write(key_file, &private_key_hex).expect("Failed to write key file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(key_file, fs::Permissions::from_mode(0o600))
            .expect("Failed to set permissions");
    }
    println!("Private key saved to: {} (mode 600)", key_file);

    // Save .env file
    let env_content = format!(
        r#"# Generated wallet for atom-intents demo
# Created: {}
#
# WARNING: This is a TESTNET key. Do not use for mainnet!
# Do not commit this file to version control.

COSMOS_PRIVATE_KEY={}
"#,
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        private_key_hex
    );

    fs::write(output_file, &env_content).expect("Failed to write env file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(output_file, fs::Permissions::from_mode(0o600))
            .expect("Failed to set permissions");
    }
    println!("Environment file saved to: {}", output_file);

    println!("\n==========================================");
    println!("  USAGE");
    println!("==========================================\n");

    println!("1. Fund your testnet wallet using a faucet:");
    println!("   - Cosmos Hub: https://faucet.testnet.cosmos.network/");
    println!("   - Osmosis: https://faucet.testnet.osmosis.zone/");
    println!("   - Neutron: https://t.me/+SyhWrlnwfCw2NGM6\n");

    println!("2. Start the demo in testnet mode:");
    println!("   source {}", output_file);
    println!("   cargo run -- --mode testnet\n");

    println!("3. Or run with the key directly:");
    println!("   COSMOS_PRIVATE_KEY=$(cat {}) cargo run -- --mode testnet\n", key_file);

    println!("IMPORTANT: Keep your private key secure!");
    println!("           Never share it or commit it to version control.");
}
