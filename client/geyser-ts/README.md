# Geyser-TS

A TypeScript client for streaming Solana data using Yellowstone gRPC Geyser plugin.

## Quick Start

### Prerequisites

- Node.js 18+
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
```

⚠️ **Please note:** This endpoint is a sample and cannot be used as is. Please obtain and configure the appropriate endpoint for your environment.

## Usage

The client will connect to the configured Yellowstone gRPC endpoint and stream Solana data.

To modify the streaming configuration, edit `src/index.ts`:

```typescript
// Configure what data to stream
const subscribeRequest = {
  // Add your subscription filters here
  accounts: {},
  slots: {},
  transactions: {},
  blocks: {},
  // ... other filters
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
- [GitHub Repository](https://github.com/elsoul/solana-stream)
