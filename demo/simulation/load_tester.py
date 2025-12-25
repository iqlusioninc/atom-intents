#!/usr/bin/env python3
"""
Load Tester for ATOM Intents Demo
Stress test the Skip Select Simulator with configurable load patterns.
"""

import argparse
import asyncio
import json
import time
from dataclasses import dataclass, field
from typing import List, Dict, Any
import aiohttp
import statistics

from intent_generator import generate_intent


@dataclass
class TestResult:
    """Result of a single request."""
    success: bool
    latency_ms: float
    status_code: int = 0
    error: str = ""
    timestamp: float = field(default_factory=time.time)


@dataclass
class TestStats:
    """Aggregated test statistics."""
    total_requests: int = 0
    successful_requests: int = 0
    failed_requests: int = 0
    latencies: List[float] = field(default_factory=list)
    errors: Dict[str, int] = field(default_factory=dict)
    start_time: float = 0
    end_time: float = 0

    @property
    def success_rate(self) -> float:
        if self.total_requests == 0:
            return 0
        return self.successful_requests / self.total_requests * 100

    @property
    def avg_latency(self) -> float:
        if not self.latencies:
            return 0
        return statistics.mean(self.latencies)

    @property
    def p50_latency(self) -> float:
        if not self.latencies:
            return 0
        return statistics.median(self.latencies)

    @property
    def p95_latency(self) -> float:
        if not self.latencies:
            return 0
        sorted_latencies = sorted(self.latencies)
        idx = int(len(sorted_latencies) * 0.95)
        return sorted_latencies[idx]

    @property
    def p99_latency(self) -> float:
        if not self.latencies:
            return 0
        sorted_latencies = sorted(self.latencies)
        idx = int(len(sorted_latencies) * 0.99)
        return sorted_latencies[min(idx, len(sorted_latencies) - 1)]

    @property
    def requests_per_second(self) -> float:
        duration = self.end_time - self.start_time
        if duration == 0:
            return 0
        return self.total_requests / duration

    def summary(self) -> str:
        duration = self.end_time - self.start_time
        return f"""
Load Test Results
=================
Duration: {duration:.2f}s
Total Requests: {self.total_requests}
Successful: {self.successful_requests} ({self.success_rate:.1f}%)
Failed: {self.failed_requests}
Requests/sec: {self.requests_per_second:.2f}

Latency (ms):
  Average: {self.avg_latency:.2f}
  P50: {self.p50_latency:.2f}
  P95: {self.p95_latency:.2f}
  P99: {self.p99_latency:.2f}
  Min: {min(self.latencies) if self.latencies else 0:.2f}
  Max: {max(self.latencies) if self.latencies else 0:.2f}

Errors:
{self._format_errors()}
"""

    def _format_errors(self) -> str:
        if not self.errors:
            return "  None"
        return "\n".join(f"  {k}: {v}" for k, v in self.errors.items())


class LoadTester:
    """Async load tester for the Skip Select API."""

    def __init__(
        self,
        target_url: str,
        requests_per_second: float,
        duration_seconds: int,
        concurrent_limit: int = 100,
    ):
        self.target_url = target_url.rstrip("/")
        self.rps = requests_per_second
        self.duration = duration_seconds
        self.concurrent_limit = concurrent_limit
        self.stats = TestStats()
        self._semaphore = asyncio.Semaphore(concurrent_limit)

    async def _make_request(
        self,
        session: aiohttp.ClientSession,
        intent: Dict[str, Any],
    ) -> TestResult:
        """Make a single API request."""
        start = time.time()
        try:
            async with self._semaphore:
                async with session.post(
                    f"{self.target_url}/api/v1/intents",
                    json=intent,
                    timeout=aiohttp.ClientTimeout(total=30),
                ) as response:
                    latency = (time.time() - start) * 1000
                    await response.text()

                    return TestResult(
                        success=response.status == 200,
                        latency_ms=latency,
                        status_code=response.status,
                    )
        except asyncio.TimeoutError:
            return TestResult(
                success=False,
                latency_ms=(time.time() - start) * 1000,
                error="timeout",
            )
        except aiohttp.ClientError as e:
            return TestResult(
                success=False,
                latency_ms=(time.time() - start) * 1000,
                error=str(type(e).__name__),
            )
        except Exception as e:
            return TestResult(
                success=False,
                latency_ms=(time.time() - start) * 1000,
                error=str(e),
            )

    async def _worker(
        self,
        session: aiohttp.ClientSession,
        results: List[TestResult],
    ):
        """Worker that sends requests at the configured rate."""
        interval = 1.0 / self.rps
        end_time = time.time() + self.duration

        while time.time() < end_time:
            intent = generate_intent()
            # Remove metadata that's not part of the API
            del intent["id"]
            del intent["metadata"]
            del intent["created_at"]

            result = await self._make_request(session, intent)
            results.append(result)

            # Sleep to maintain RPS
            await asyncio.sleep(interval)

    async def run(self) -> TestStats:
        """Run the load test."""
        print(f"Starting load test against {self.target_url}")
        print(f"Target: {self.rps} requests/sec for {self.duration} seconds")
        print(f"Concurrent limit: {self.concurrent_limit}")
        print()

        results: List[TestResult] = []
        self.stats = TestStats()
        self.stats.start_time = time.time()

        # Create worker tasks
        connector = aiohttp.TCPConnector(limit=self.concurrent_limit)
        async with aiohttp.ClientSession(connector=connector) as session:
            # Use multiple workers to achieve higher RPS
            num_workers = min(int(self.rps / 10) + 1, 50)
            worker_rps = self.rps / num_workers

            # Override the instance RPS for workers
            original_rps = self.rps
            self.rps = worker_rps

            tasks = [
                asyncio.create_task(self._worker(session, results))
                for _ in range(num_workers)
            ]

            # Progress reporting
            async def report_progress():
                while True:
                    await asyncio.sleep(5)
                    print(f"Progress: {len(results)} requests sent...")

            progress_task = asyncio.create_task(report_progress())

            try:
                await asyncio.gather(*tasks)
            finally:
                progress_task.cancel()
                try:
                    await progress_task
                except asyncio.CancelledError:
                    pass

            self.rps = original_rps

        self.stats.end_time = time.time()

        # Aggregate results
        for result in results:
            self.stats.total_requests += 1
            self.stats.latencies.append(result.latency_ms)

            if result.success:
                self.stats.successful_requests += 1
            else:
                self.stats.failed_requests += 1
                error_key = result.error or f"HTTP {result.status_code}"
                self.stats.errors[error_key] = self.stats.errors.get(error_key, 0) + 1

        return self.stats


async def main_async(args):
    tester = LoadTester(
        target_url=args.target,
        requests_per_second=args.rps,
        duration_seconds=args.duration,
        concurrent_limit=args.concurrent,
    )

    stats = await tester.run()
    print(stats.summary())

    if args.output:
        with open(args.output, "w") as f:
            json.dump({
                "config": {
                    "target": args.target,
                    "rps": args.rps,
                    "duration": args.duration,
                    "concurrent": args.concurrent,
                },
                "results": {
                    "total_requests": stats.total_requests,
                    "successful_requests": stats.successful_requests,
                    "failed_requests": stats.failed_requests,
                    "success_rate": stats.success_rate,
                    "requests_per_second": stats.requests_per_second,
                    "latency_avg_ms": stats.avg_latency,
                    "latency_p50_ms": stats.p50_latency,
                    "latency_p95_ms": stats.p95_latency,
                    "latency_p99_ms": stats.p99_latency,
                    "errors": stats.errors,
                },
            }, f, indent=2)
        print(f"Results written to {args.output}")


def main():
    parser = argparse.ArgumentParser(description="Load test the Skip Select Simulator")
    parser.add_argument("--target", type=str, default="http://localhost:8080",
                        help="Target URL")
    parser.add_argument("--rps", type=float, default=10,
                        help="Requests per second")
    parser.add_argument("--duration", type=int, default=60,
                        help="Test duration in seconds")
    parser.add_argument("--concurrent", type=int, default=100,
                        help="Maximum concurrent requests")
    parser.add_argument("--output", type=str, help="Output file for results (JSON)")

    args = parser.parse_args()

    asyncio.run(main_async(args))


if __name__ == "__main__":
    main()
