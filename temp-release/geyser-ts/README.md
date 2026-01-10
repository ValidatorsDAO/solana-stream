# Geyser-TS

A TypeScript client for streaming Solana data using Yellowstone gRPC Geyser plugin.

## Quick Start

### Prerequisites

- Node.js 22 or 24 (LTS)
- pnpm (recommended) or npm
- Access to a Yellowstone gRPC endpoint

### Installation

1. Clone or download this project
2. Install dependencies:

```bash
pnpm install
# or
npm install
```

3. Set up environment variables:

```bash
cp .env.example .env
# Edit .env with your configuration
```

4. Run the client:

```bash
pnpm dev
# or
npm run dev
```

## Configuration

Create a `.env` file with the following configuration:

```env
X_TOKEN=your_token_here
GEYSER_ENDPOINT=https://grpc-ams.erpc.global
SOLANA_RPC_ENDPOINT="https://edge.erpc.global?api-key=YOUR_API_KEY"
```

⚠️ **Please note:** This endpoint is a sample and cannot be used as is. Please obtain and configure the appropriate endpoint for your environment.

## Optional Runtime Settings

Enable metrics/drop/subscription logs with environment flags:

```env
# Log periodic metrics (queue size, rates, drops)
GEYSER_LOG_METRICS=1

# Log drop warnings when backpressure kicks in
GEYSER_LOG_DROPS=1

# Log when subscription requests are sent
GEYSER_LOG_SUBSCRIBE=1
```

## Production-Ready Best Practices

- Ping/Pong handling to keep Yellowstone gRPC streams alive
- Exponential reconnect backoff plus `fromSlot` gap recovery
- Bounded in-memory queue with drop logging for backpressure safety
- Code-based subscription filters in TypeScript (`src/utils/filter.ts`)
- Optional runtime metrics logging (rates, queue size, drops)
- Default filters drop vote/failed transactions to reduce traffic

Tip: start with slots, then add filters as needed. When resuming from `fromSlot`,
duplicates are expected.

## Usage

The client will connect to the configured Yellowstone gRPC endpoint and stream Solana data.

## Notes on Meta/Events

- Shreds-only pipelines can decode instruction data but do not include execution meta/logs.
- For exact fills or program events, use Yellowstone gRPC transaction updates and/or RPC `getTransaction` (confirmed/finalized).

To customize streaming and trading logic, edit these files:

- `src/index.ts`: `onUpdate`/`onTransaction`/`onAccount` hooks (add trading logic here)
- `src/utils/filter.ts`: subscription filters (TypeScript, `CommitmentLevel` enums)
- `src/handlers/logUpdate.ts`: console logging helpers (optional)
- `src/handlers/latency.ts`: latency tracking helper

Example hook:

```typescript
const onTransaction = (transactionUpdate: any) => {
  // TODO: Add your trade logic here.
}
```

## Dependencies

This project uses:

- `@solana/yellowstone-grpc` - Main gRPC client for Yellowstone
- `@solana/web3.js` - Solana JavaScript SDK
- `dotenv` - Environment variable loading

## Scripts

- `pnpm dev` - Run in development mode
- `pnpm build` - Build for production
- `pnpm start` - Run built version

## Example Output

The client will log streaming data from the Solana blockchain including accounts, transactions, slots, and blocks based on your subscription configuration.

## Development

Build the project:

```bash
pnpm build
```

Run in development mode with hot reload:

```bash
pnpm dev
```

## License

MIT License

## More Information

For more details about Yellowstone gRPC and Solana streaming:

- [Yellowstone gRPC Documentation](https://github.com/rpcpool/yellowstone-grpc)
- [Solana Web3.js Documentation](https://solana-labs.github.io/solana-web3.js/)
- [GitHub Repository](https://github.com/ValidatorsDAO/solana-stream)
