# cardano-slurp

Connects to one or more cardano-node's, streams all available transactions, and saves them to disk (or to S3) in raw cbor format.

## Usage

Aims to have sensible defaults; Running cardano-slurp without arguments will connect to an IOHK relay and save blocks to the `blocks` directory

```shell
cardano-slurp
```

You can specify custom values via command line or environment variable:

```shell
cargo-slurp --relay relays.cardano-mainnet.iohk.io:3001 --directory db

RELAY=relays-new.cardano-mainnet.iohk.io:3001 cargo-slurp
``` 

## Format

The file structure after running (assuming default parameters) should look like this:
```
 - blocks                | Contains all persisted data
   - headers             | All downloaded headers
     - {start-slot}      | The starting slot for a batch of file headers, to cut down on files per directory
       - {slot}-{hash}   | The header we observed at {slot} with the given {hash}; there may be multiples in the case of rollbacks or different blocks received from different relays
   - bodies              | All downloaded block bodies 
     - {start-slot}      | The starting slot for a batch of file bodies, to cut down on files per directory
       - {slot}-{hash}   | The block body we observed at {slot} with the given {hash}; there may be multiples in the case of rollbacks or different blocks received from different relays

## Background

On 2023-01-22, at 1:09:01 UTC, nearly 60% of all cardano-node's, all nodes with incoming connections, [crashed.](https://github.com/input-output-hk/cardano-node/issues/4826)

This was likely caused by some kind of radioactive data, which propogated through the network and caused a crash before being persisted to the immutable database. After rebooting, the network continued where it left off, producing blocks on the last known tip.

However, because of this, we have no record of the "ephemeral" data that may have caused this issue, making it difficult to track down the issue.

To defend against this in the future, and to open up some interesting data-analytics use cases, this project seeks to archive the totality of cardano block history, not just the chain history.