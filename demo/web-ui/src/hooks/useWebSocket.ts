import { useEffect, useRef, useCallback, useState } from 'react';
import type { WsMessage, Intent, Auction, SolverQuote, Settlement, PriceFeed, SystemStats } from '../types';

interface WebSocketState {
  connected: boolean;
  lastMessage: WsMessage | null;
}

interface UseWebSocketReturn extends WebSocketState {
  send: (message: unknown) => void;
}

export function useWebSocket(
  onIntent?: (intent: Intent) => void,
  onAuction?: (auction: Auction) => void,
  onQuote?: (quote: SolverQuote) => void,
  onSettlement?: (settlement: Settlement) => void,
  onPrices?: (prices: PriceFeed[]) => void,
  onStats?: (stats: SystemStats) => void
): UseWebSocketReturn {
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout>>();
  const [state, setState] = useState<WebSocketState>({
    connected: false,
    lastMessage: null,
  });

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      return;
    }

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;

    try {
      const ws = new WebSocket(wsUrl);

      ws.onopen = () => {
        console.log('WebSocket connected');
        setState((s) => ({ ...s, connected: true }));

        // Subscribe to all topics
        ws.send(
          JSON.stringify({
            type: 'subscribe',
            data: { topics: ['intents', 'auctions', 'settlements', 'prices', 'stats'] },
          })
        );
      };

      ws.onmessage = (event) => {
        try {
          const message: WsMessage = JSON.parse(event.data);
          setState((s) => ({ ...s, lastMessage: message }));

          // Dispatch to handlers
          switch (message.type) {
            case 'intent_submitted':
              onIntent?.(message.data);
              break;
            case 'auction_started':
            case 'auction_completed':
              onAuction?.(message.data);
              break;
            case 'quote_received':
              onQuote?.(message.data);
              break;
            case 'settlement_update':
              onSettlement?.(message.data);
              break;
            case 'price_update':
              onPrices?.(message.data);
              break;
            case 'stats_update':
              onStats?.(message.data);
              break;
          }
        } catch (e) {
          console.error('Failed to parse WebSocket message:', e);
        }
      };

      ws.onclose = () => {
        console.log('WebSocket disconnected');
        setState((s) => ({ ...s, connected: false }));
        wsRef.current = null;

        // Attempt to reconnect after 2 seconds
        reconnectTimeoutRef.current = setTimeout(connect, 2000);
      };

      ws.onerror = (error) => {
        console.error('WebSocket error:', error);
      };

      wsRef.current = ws;
    } catch (e) {
      console.error('Failed to create WebSocket:', e);
      reconnectTimeoutRef.current = setTimeout(connect, 2000);
    }
  }, [onIntent, onAuction, onQuote, onSettlement, onPrices, onStats]);

  useEffect(() => {
    connect();

    return () => {
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      if (wsRef.current) {
        wsRef.current.close();
      }
    };
  }, [connect]);

  const send = useCallback((message: unknown) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(message));
    }
  }, []);

  return {
    ...state,
    send,
  };
}
