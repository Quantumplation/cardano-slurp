# cardano-slurp

Connects to one or more cardano-node's, streams all available transactions, and saves them to disk (or to S3) in raw cbor format.

## Usage

Aims to have sensible defaults; Running cardano-slurp without arguments will connect to an IOHK relay and save blocks to the `blocks` directory

```shell
cardano-slurp
```

You can specify custom values via command line or environment variable:

```shell
$ cardano-slurp --help

Connect to cardano nodes and download all blocks and transactions without processing them

Usage: cardano-slurp [OPTIONS]

Options:
  -r, --relay <RELAY>
          The cardano relay node to connect to [default: relays-new.cardano-mainnet.iohk.io:3001]
  -t, --topology-file <TOPOLOGY_FILE>
          A topology file to read for relays to connect to
  -f, --fallback-point <FALLBACK_POINT>
          
  -d, --directory <DIRECTORY>
          The directory to save blocks into [default: db]
      --testnet-magic <TESTNET_MAGIC>
          The network magic to use when communicating with nodes
  -h, --help
          Print help
  -V, --version
          Print version

cardano-slurp --relay relays.cardano-mainnet.iohk.io:3001 --directory db --fallback-point 78416/f85c52e97c6ec4e171d92789e32331e624ee7a0c7ba18b578062727edb7d61f7

RELAY=relays-new.cardano-mainnet.iohk.io:3001 cargo-slurp
```

Rather than specifying relays individually, you can specify a topology.json file in the same format that the cardano-node reads:

```shell
cardano-slurp --topology-file topology.json
```

## Format

The file structure after running (assuming default parameters) should look like this:
```
 - db                    | Contains all persisted data
   - headers             | All downloaded headers
     - {large-bucket}    | See note on bucketing below
       - {small-bucket}  |
         - {slot}-{hash} | The header we observed at {slot} with the given {hash}; there may be multiples in the case of rollbacks or different blocks received from different relays
   - bodies              | All downloaded block bodies 
     - {large-bucket}    | See note on bucketing below
       - {small-bucket}  |
         - {slot}-{hash} | The block body we observed at {slot} with the given {hash}; there may be multiples in the case of rollbacks or different blocks received from different relays
   - cursors             | Cursors, tracking how far we've sync'd with any given relay
    - {relay}            | The cursor file, serialized as CBOR
```

> NOTE: Common wisdom seems to indicate that you should keep directories to around 10k entries so as not to destroy performance of directory scan operations.  Thus, we introduce two layers of nesting, called buckets, to occasionally roll over to an empty directory and keep the sizes small.  Each bucket represents the starting slot of a range which contains all the blocks in that subdirectory.  The large bucket rolls over ever 20 million slots, and the small bucket rolls over every 200 thousand slots.  This ensures that each large-bucket directory has no more than 1000 entries, and each small-bucket directory has no more than 10,000 entries.  One large-bucket represnets roughly 230 days of blocks in the shelley era. 

