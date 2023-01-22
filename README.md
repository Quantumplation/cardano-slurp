# cardano-slurp

Connects to one or more cardano-node's, streams all available transactions, and saves them to disk (or to S3) in raw cbor format.

## Background

On 2023-01-22, at 1:09:01 UTC, nearly 60% of all cardano-node's, all nodes with incoming connections, [crashed.](https://github.com/input-output-hk/cardano-node/issues/4826)

This was likely caused by some kind of radioactive data, which propogated through the network and caused a crash before being persisted to the immutable database. After rebooting, the network continued where it left off, producing blocks on the last known tip.

However, because of this, we have no record of the "ephemeral" data that may have caused this issue, making it difficult to track down the issue.

To defend against this in the future, and to open up some interesting data-analytics use cases, this project seeks to archive the totality of cardano block history, not just the chain history.