#!/bin/bash
# Generate a new Cosmos wallet for testnet usage
#
# This script generates a secp256k1 private key and derives
# the corresponding Cosmos addresses for different chains.

set -e

# Output file for the key (default: .env.local)
OUTPUT_FILE="${1:-.env.local}"
KEY_FILE="${2:-wallet.key}"

echo "Generating new Cosmos wallet..."

# Generate 32 random bytes as hex (64 hex characters)
if command -v openssl &> /dev/null; then
    PRIVATE_KEY=$(openssl rand -hex 32)
elif command -v xxd &> /dev/null; then
    PRIVATE_KEY=$(head -c 32 /dev/urandom | xxd -p -c 64)
else
    echo "Error: Need either openssl or xxd to generate random bytes"
    exit 1
fi

echo ""
echo "=========================================="
echo "  NEW WALLET GENERATED"
echo "=========================================="
echo ""
echo "Private Key (hex):"
echo "  $PRIVATE_KEY"
echo ""
echo "IMPORTANT: Save this key securely!"
echo "           Anyone with this key can control the wallet."
echo ""

# Save to key file
echo "$PRIVATE_KEY" > "$KEY_FILE"
chmod 600 "$KEY_FILE"
echo "Private key saved to: $KEY_FILE (mode 600)"

# Save to .env file for the demo
cat > "$OUTPUT_FILE" << EOF
# Generated wallet for atom-intents demo
# Created: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
#
# WARNING: This is a TESTNET key. Do not use for mainnet!
# Do not commit this file to version control.

COSMOS_PRIVATE_KEY=$PRIVATE_KEY
EOF

chmod 600 "$OUTPUT_FILE"
echo "Environment file saved to: $OUTPUT_FILE"
echo ""

# Try to derive addresses if cargo is available and the project compiles
if command -v cargo &> /dev/null; then
    echo "Deriving addresses..."
    echo ""

    # Run a quick Rust snippet to derive addresses
    cd "$(dirname "$0")/.."

    # Create a temporary bin to derive addresses
    cat > /tmp/derive_address.rs << 'RUSTCODE'
use k256::ecdsa::SigningKey;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: derive_address <hex_private_key>");
        std::process::exit(1);
    }

    let key_hex = &args[1];
    let key_bytes = hex::decode(key_hex).expect("Invalid hex");
    let signing_key = SigningKey::from_bytes((&key_bytes[..]).into()).expect("Invalid key");
    let verifying_key = signing_key.verifying_key();
    let pubkey_bytes = verifying_key.to_sec1_bytes();

    // SHA256 -> RIPEMD160
    let sha256_hash = Sha256::digest(&pubkey_bytes);
    let ripemd_hash = Ripemd160::digest(&sha256_hash);

    // Derive addresses for different chains
    let prefixes = vec![
        ("cosmos", "Cosmos Hub"),
        ("osmo", "Osmosis"),
        ("neutron", "Neutron"),
        ("celestia", "Celestia"),
    ];

    for (prefix, name) in prefixes {
        let hrp = bech32::Hrp::parse(prefix).unwrap();
        let address = bech32::encode::<bech32::Bech32>(hrp, ripemd_hash.as_slice()).unwrap();
        println!("  {} ({}): {}", name, prefix, address);
    }
}
RUSTCODE

    # Try to compile and run, but don't fail if it doesn't work
    if cargo build --quiet 2>/dev/null; then
        # Use the project's dependencies to run the derivation
        cargo run --quiet --example derive_address -- "$PRIVATE_KEY" 2>/dev/null || {
            echo "  (Address derivation requires running the full binary)"
            echo "  Start the demo with: cargo run -- --mode testnet"
            echo "  The address will be logged on startup."
        }
    fi

    rm -f /tmp/derive_address.rs
fi

echo ""
echo "=========================================="
echo "  USAGE"
echo "=========================================="
echo ""
echo "1. Fund your testnet wallet using a faucet:"
echo "   - Cosmos Hub Testnet: https://faucet.testnet.cosmos.network/"
echo "   - Osmosis Testnet: https://faucet.testnet.osmosis.zone/"
echo "   - Neutron Testnet: https://t.me/+SyhWrlnwfCw2NGM6"
echo ""
echo "2. Start the demo in testnet mode:"
echo "   source $OUTPUT_FILE"
echo "   cargo run -- --mode testnet"
echo ""
echo "3. Or run with the key directly:"
echo "   COSMOS_PRIVATE_KEY=\$(cat $KEY_FILE) cargo run -- --mode testnet"
echo ""
