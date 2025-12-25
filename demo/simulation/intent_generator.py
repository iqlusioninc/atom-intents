#!/usr/bin/env python3
"""
Intent Generator for ATOM Intents Demo
Generates realistic trading intents for simulation and testing.
"""

import argparse
import json
import random
import time
from datetime import datetime, timedelta
from typing import Dict, List, Any
import uuid

# Token configuration
TOKENS = {
    "ATOM": {"chain": "cosmoshub-4", "price_usd": 9.50, "volatility": 0.03},
    "OSMO": {"chain": "osmosis-1", "price_usd": 0.65, "volatility": 0.04},
    "USDC": {"chain": "noble-1", "price_usd": 1.00, "volatility": 0.001},
    "NTRN": {"chain": "neutron-1", "price_usd": 0.45, "volatility": 0.05},
    "STRD": {"chain": "stride-1", "price_usd": 1.20, "volatility": 0.04},
}

# Trading pair configurations with volume weights
TRADING_PAIRS = [
    ("ATOM", "OSMO", 0.25),
    ("OSMO", "ATOM", 0.20),
    ("ATOM", "USDC", 0.20),
    ("USDC", "ATOM", 0.15),
    ("NTRN", "ATOM", 0.10),
    ("ATOM", "NTRN", 0.05),
    ("OSMO", "USDC", 0.05),
]

# User behavior profiles
USER_PROFILES = [
    {
        "name": "retail",
        "weight": 0.70,
        "amount_range": (10, 500),  # USD
        "slippage_tolerance": (0.5, 2.0),
        "timeout": (30, 120),
    },
    {
        "name": "trader",
        "weight": 0.20,
        "amount_range": (500, 10000),
        "slippage_tolerance": (0.1, 0.5),
        "timeout": (15, 60),
    },
    {
        "name": "whale",
        "weight": 0.10,
        "amount_range": (10000, 1000000),
        "slippage_tolerance": (0.2, 1.0),
        "timeout": (60, 300),
    },
]


def generate_address() -> str:
    """Generate a random Cosmos address."""
    return f"cosmos1{''.join(random.choices('0123456789abcdef', k=38))}"


def select_trading_pair() -> tuple:
    """Select a trading pair based on volume weights."""
    weights = [p[2] for p in TRADING_PAIRS]
    selected = random.choices(TRADING_PAIRS, weights=weights, k=1)[0]
    return selected[0], selected[1]


def select_user_profile() -> Dict:
    """Select a user profile based on weights."""
    weights = [p["weight"] for p in USER_PROFILES]
    return random.choices(USER_PROFILES, weights=weights, k=1)[0]


def generate_intent(
    timestamp: datetime = None,
    price_deviation: float = 0.0,
) -> Dict[str, Any]:
    """Generate a single trading intent."""
    if timestamp is None:
        timestamp = datetime.utcnow()

    # Select trading pair and profile
    input_denom, output_denom = select_trading_pair()
    profile = select_user_profile()

    # Calculate amounts
    input_token = TOKENS[input_denom]
    output_token = TOKENS[output_denom]

    # Generate USD amount based on profile
    usd_amount = random.uniform(*profile["amount_range"])

    # Apply price with deviation
    input_price = input_token["price_usd"] * (1 + price_deviation * random.uniform(-1, 1))
    output_price = output_token["price_usd"] * (1 + price_deviation * random.uniform(-1, 1))

    # Calculate token amounts (in micro units)
    input_amount = int((usd_amount / input_price) * 1_000_000)
    expected_output = int((usd_amount / output_price) * 1_000_000)

    # Apply slippage tolerance
    slippage = random.uniform(*profile["slippage_tolerance"]) / 100
    min_output = int(expected_output * (1 - slippage))

    # Generate timeout
    timeout_seconds = random.randint(*profile["timeout"])

    return {
        "id": f"intent_{uuid.uuid4()}",
        "user_address": generate_address(),
        "input": {
            "chain_id": input_token["chain"],
            "denom": input_denom,
            "amount": input_amount,
        },
        "output": {
            "chain_id": output_token["chain"],
            "denom": output_denom,
            "min_amount": min_output,
        },
        "fill_config": {
            "allow_partial": random.random() > 0.3,
            "min_fill_percent": random.choice([50, 75, 80, 90, 100]),
            "strategy": random.choice(["eager", "all_or_nothing", "price_based"]),
        },
        "constraints": {
            "max_hops": random.choice([2, 3, 4]),
            "allowed_venues": [],
            "excluded_venues": [],
            "max_slippage_bps": int(slippage * 10000),
        },
        "timeout_seconds": timeout_seconds,
        "created_at": timestamp.isoformat() + "Z",
        "metadata": {
            "profile": profile["name"],
            "usd_value": round(usd_amount, 2),
            "expected_output": expected_output,
        },
    }


def generate_matching_pair() -> tuple:
    """Generate two opposing intents that can be matched."""
    timestamp = datetime.utcnow()

    # Generate first intent
    intent1 = generate_intent(timestamp)

    # Generate opposing intent
    intent2 = generate_intent(timestamp)
    intent2["input"]["denom"] = intent1["output"]["denom"]
    intent2["input"]["chain_id"] = intent1["output"]["chain_id"]
    intent2["output"]["denom"] = intent1["input"]["denom"]
    intent2["output"]["chain_id"] = intent1["input"]["chain_id"]

    # Adjust amounts to be similar
    intent2["input"]["amount"] = int(intent1["output"]["min_amount"] * random.uniform(0.9, 1.1))
    intent2["output"]["min_amount"] = int(intent1["input"]["amount"] * random.uniform(0.85, 0.95))

    return intent1, intent2


def generate_batch(
    count: int,
    matching_ratio: float = 0.3,
    time_span_seconds: int = 60,
) -> List[Dict]:
    """Generate a batch of intents with optional matching pairs."""
    intents = []
    start_time = datetime.utcnow()

    # Generate matching pairs
    matching_count = int(count * matching_ratio / 2)
    for _ in range(matching_count):
        offset = random.uniform(0, time_span_seconds)
        timestamp = start_time + timedelta(seconds=offset)
        intent1, intent2 = generate_matching_pair()
        intent1["created_at"] = timestamp.isoformat() + "Z"
        intent2["created_at"] = (timestamp + timedelta(seconds=random.uniform(0, 5))).isoformat() + "Z"
        intents.extend([intent1, intent2])

    # Generate remaining random intents
    remaining = count - len(intents)
    for _ in range(remaining):
        offset = random.uniform(0, time_span_seconds)
        timestamp = start_time + timedelta(seconds=offset)
        intents.append(generate_intent(timestamp))

    # Sort by timestamp
    intents.sort(key=lambda x: x["created_at"])

    return intents


def main():
    parser = argparse.ArgumentParser(description="Generate trading intents for simulation")
    parser.add_argument("--count", type=int, default=100, help="Number of intents to generate")
    parser.add_argument("--matching-ratio", type=float, default=0.3,
                        help="Ratio of intents that can be matched (0-1)")
    parser.add_argument("--time-span", type=int, default=60,
                        help="Time span in seconds for intent generation")
    parser.add_argument("--output", type=str, default="intents.json",
                        help="Output file path")
    parser.add_argument("--pretty", action="store_true", help="Pretty print JSON output")

    args = parser.parse_args()

    print(f"Generating {args.count} intents...")
    intents = generate_batch(
        count=args.count,
        matching_ratio=args.matching_ratio,
        time_span_seconds=args.time_span,
    )

    # Calculate statistics
    total_volume = sum(i["metadata"]["usd_value"] for i in intents)
    profile_counts = {}
    for intent in intents:
        profile = intent["metadata"]["profile"]
        profile_counts[profile] = profile_counts.get(profile, 0) + 1

    print(f"Generated {len(intents)} intents")
    print(f"Total volume: ${total_volume:,.2f}")
    print(f"Profile distribution: {profile_counts}")

    # Write output
    with open(args.output, "w") as f:
        if args.pretty:
            json.dump(intents, f, indent=2)
        else:
            json.dump(intents, f)

    print(f"Written to {args.output}")


if __name__ == "__main__":
    main()
