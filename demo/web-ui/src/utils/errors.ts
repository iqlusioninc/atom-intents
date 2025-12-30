/**
 * User-friendly error message mapping
 * Converts technical errors into actionable messages for users
 */

const ERROR_PATTERNS: Array<{ pattern: RegExp; message: string }> = [
  {
    pattern: /missing field/i,
    message: 'Contract message format error. Please refresh the page and try again.',
  },
  {
    pattern: /insufficient funds/i,
    message: 'Insufficient funds for this transaction.',
  },
  {
    pattern: /sequence mismatch|account sequence/i,
    message: 'Transaction conflict detected. Please wait a moment and try again.',
  },
  {
    pattern: /429|rate limit|too many requests/i,
    message: 'Server is busy. Please wait a few seconds and try again.',
  },
  {
    pattern: /timeout|timed out|ETIMEDOUT/i,
    message: 'Network timeout. Please check your connection and try again.',
  },
  {
    pattern: /rejected|denied|cancelled/i,
    message: 'Transaction was cancelled.',
  },
  {
    pattern: /not found|does not exist/i,
    message: 'Resource not found. Please refresh the page.',
  },
  {
    pattern: /insufficient gas/i,
    message: 'Transaction ran out of gas. Please try again.',
  },
  {
    pattern: /connection refused|network error|fetch failed/i,
    message: 'Network error. Please check your connection.',
  },
  {
    pattern: /keplr not installed|please install keplr/i,
    message: 'Please install the Keplr wallet extension to continue.',
  },
  {
    pattern: /wallet not connected/i,
    message: 'Please connect your wallet first.',
  },
];

/**
 * Convert a technical error into a user-friendly message
 */
export function friendlyError(error: unknown): string {
  const rawMessage = error instanceof Error ? error.message : String(error);

  for (const { pattern, message } of ERROR_PATTERNS) {
    if (pattern.test(rawMessage)) {
      return message;
    }
  }

  // Fallback for unrecognized errors
  return 'Something went wrong. Please try again.';
}

/**
 * Get both the friendly message and the technical details
 * Useful for showing a friendly message with a "details" toggle
 */
export function errorWithDetails(error: unknown): {
  friendly: string;
  technical: string;
} {
  const technical = error instanceof Error ? error.message : String(error);
  return {
    friendly: friendlyError(error),
    technical,
  };
}
