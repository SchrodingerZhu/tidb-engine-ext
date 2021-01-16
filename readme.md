# RaftStoreProxy

- Author(s):     [Tong Zhigao](https://github.com/solotzg)
- Last updated:  2021-1-9

## Abstract

This proposal is to introduce a TiKV based `c dynamic library` for extending storage system in `TiDB` cluster. This library aims to export current multi-raft framework to other engines and make them be able to provide services(read/write) as raftstore directly.

<!--
A short summary of the proposal:
- What is the issue that the proposal aims to solve?
- What needs to be done in this proposal?
- What is the impact of this proposal?
-->

## Background

Initially, I came up with such idea while designing the architecture of `TiFlash`(a distributed column-based storage for `Realtime HTAP` scene).

There is already a distributed OLTP storage product `TiKV`, and we can extern other kind of realtime analytics system based on current multi-raft mechanism to handle more complicated scenes. For example, assume strong schema-aware storage nodes could be accessed as raftstore with special identification labels. Third-party components can use [Placement Rules](https://docs.pingcap.com/tidb/stable/configure-placement-rules), provided by `PD`, to schedule learner/voter replica on them. If such storage system has supported `Multi-raft FSM`, `Percolator Transaction Model` and `Coprocessor Protocol`, just like TiFlash does, it will be be appropriate for `Realtime HTAP` scene. If transaction is not required, like most OLAP cases which only guarantee `Eventual Consistency`, and what matters more is throughput rather than latency. Then, data(formed by table schema or other pattern) could be R/W from this kind of raftstore directly.

For this purpose, it's necessary to implement a multi-raft library to help integrate other system into raftstore. 

<!--
An introduction of the necessary background and the problem being solved by the proposed change:
- The drawback of the current feature and the corresponding use case
- The expected outcome of this proposal.
-->

## Proposal

### Overview

In order to be compatible with process of TiKV and reduce risks about corner cases, such as dealing with snapshot and region merge, there is no need to reimplement from scratch. It's feasible to refactor TiKV source code and extract parts of necessary process into interfaces since this framework is not designed for OLTP scene. The main categories are like:

- apply normal write raft command
- apply admin raft command
- peer detect: create/destroy peer
- ingest sst
- store stats: key/bytes R/W stats; disk stats; storage engine stats;
- snapshot: apply TiKV/self-defined snapshot; generate/serialize self-defined snapshot;
- region stats: approximate size/keys
- region split: scan split keys
- replica read: region read index
- encryption: get file; new file; delete file; link file; rename file;
- status services: metrics; cpu profile; self-defined stats;

Generally speaking, there are two storage components, `RaftEngine` and `KvEngine`, in TiKV for maintaining multi-raft RSM(Replicated State Machine). KvEngine is mainly used for applying raft command and providing key-value services. Each time raft log has been committed in RaftEngine, it will be parsed into normal/admin raft command and be handled by apply process. Multiple modifications about region data/meta/apply-state will be encapsulated into one `Write Batch` and written into KvEngine atomically. Maybe replacing KvEngine an option, but for other storage system, it's not easy to guarantee atomicity while writing or reading dynamic key-value pair(such as meta/apply-state) and patterned data(strong schema) together. Besides, there are a few modules and components(like importer or lighting) reply on the sst format of KvEngine. It may take a lot of cost to achieve such replacing.

In order to bring few intrusive modifications against original logic of TiKV, it's suggested to let apply process work as usual but only persist meta and state information. It means each place, where may write normal region data, must be replaced with related interfaces. Not like KvEngine, storage system under such framework, should be aware of transition about multi-raft state machine from these interfaces and must have ability about dealing with raft commands, so as to handle queries with region epoch. 

Anyway, there are at least two asynchronous runtimes in one program, therefore, the best practice of such raftstore is to guarantee `External Consistency`. Actually, the raft log persisted in RaftEngine is the `WAL(Write Ahead Log)` of apply processes. If process is interrupted at middle step, it should replay from last persisted apply-index after restarted, and related modifications cannot be witnessed from outside until meets check-point. When any peer tries to respond queries, it should get latest committed-index from leader and wait until apply-index caught up, in order to make sure it has had enough context. No matter for learner/follower or even leader, `Read Index` is a good choice to check latest `Lease`, because it's easy to make any peer of region group provide read service under same logic as long as overhead of read-index(quite light rpc call) itself is insignificant.

`Idempotency` is also an important property, which means such system could handle outdated raft commands. An optional way is like:

- fsync important meta/data when necessary
- tell and ignore commands with outdated apply-index
- recover from any middle step by overwriting

Since the program language `Rust`, which TiKV is based on, has zero-cost abstractions. It's very easy to let different processes interact with almost little cost by `FFI`(Foreign Function Interface). But any caller must be quite clear about the boundary of safe/unsafe operations exactly and make sure the interfaces, used by different runtimes, must have same memory layout.

### Direct Writing

To support direct writing without transaction, storage system must implement related interfaces about behavior of leader:

- scan range of a region to generate keys for split if reach threshold
- get approximate size/keys information of a region
- report R/W stats
- snapshot
  - generate self-defined snapshot and serialize into file
  - apply snapshot from TiKV or self-defined storage

Then, `Placement Rules` could be used to let PD transfer region groups under specific ranges from TiKV to raftstores with special labels(such as "engine":"xxxx").

### Other Features

Function about `IngestSST` is the core to be compatible with `TiDB Lighting` and `BR` for HTAP scene. It can substantially speed up data loading/restoring, but is not suitable for direct writing. Besides, the data format of this kind of SST is defined by TiKV, which might be strongly depended by other components, and it's not easy to support self-defined storage. At the same time, modules about `CDC` and `BR` are no longer needed and should be removed to avoid affecting related components in cluster.

Encryption is also an important feature for `DBaaS`(database as a service). To be compatible with TiKV, a data key manager with same logic is indispensable, especially for rotating data encryption key or using KMS service.

Status services like metrics, cpu/mem profile(flame graph) or other self-defined stats can provide effective support for locating performance bottlenecks. It's suggested to encapsulate those into one status server and let other external components visit through status address. Most of original metrics of TiKV could be reused and an optional way is to add specific prefix for each name.

<!--
A precise statement of the proposed change:
- The new named concepts and a set of metrics to be collected in this proposal (if applicable)
- The overview of the design.
- How it works?
- What needs to be changed to implement this design?
- What may be positively influenced by the proposed change?
- What may be negatively impacted by the proposed change?
-->

## Rationale

There haven't been any attempt to abstract relevant layers about multi-raft state machine of TiKV and make a library to help extend self-defined raftstore. Obviously, such library is not designed to provide comprehensive supports for OLTP scene equivalent to TiKV. The main battlefields of it are about HTAP or OLAP. Other storage systems do not need to consider about distributed fault tolerance/recovery or automatic re-balancing, because the library itself has inherited these important features from TiKV. 

<!--
A discussion of alternate approaches and the trade-offs, advantages, and disadvantages of the specified approach:
- How other systems solve the same issue?
- What other designs have been considered and what are their disadvantages?
- What is the advantage of this design compared with other designs?
- What is the disadvantage of this design?
- What is the impact of not doing this?
-->


<!--
## Compatibility and Migration Plan

A discussion of the change with regard to the compatibility issues:
- Does this proposal make TiDB not compatible with the old versions?
- Does this proposal make TiDB not compatible with TiDB tools?
    + [BR](https://github.com/pingcap/br)
    + [DM](https://github.com/pingcap/dm)
    + [Dumpling](https://github.com/pingcap/dumpling)
    + [TiCDC](https://github.com/pingcap/ticdc)
    + [TiDB Binlog](https://github.com/pingcap/tidb-binlog)
    + [TiDB Lightning](https://github.com/pingcap/tidb-lightning)
- If the existing behavior will be changed, how will we phase out the older behavior?
- Does this proposal make TiDB more compatible with MySQL?
- What is the impact(if any) on the data migration:
    + from MySQL to TiDB
    + from TiDB to MySQL
    + from old TiDB cluster to new TiDB cluster
-->

## Implementation

There are total two exposed functions:

- print-version: print necessary version information(just like TiKV does) into standard output.
- main-entry:
  - refactor the main entry of TiKV into a `extern "C"` function, and let caller input established function pointers which will be invoked dynamically.
  - main entry function is suggested to be run in another independent thread because it will block current context.

Each corresponding module listed above should be refactored to interact with external storage system. Label with key "engine" is forbidden in dynamic config and should be specified when compiling.

<!--
A detailed description for each step in the implementation:
- Does any former steps block this step?
- Who will do it?
- When to do it?
- How long it takes to accomplish it?
-->

## Testing Plan

A demo is needed to show how this library works:

- use this library to implement a self-defined raftstore, which can be called `test_engine`.
- deploy several `test_engine` with label "engine":"test_engine" in a TiDB cluster.
- design tools to show whether `test_engine` can provide services as normal raftstore.
  - transfer some region groups to `test_engine` by setting placement rules to PD.
  - write data with specific schema to `test_engine` then read through self-defined coprocessor interfaces.
  - compare performance with TiDB in some OLAP cases.
  - check distributed fault tolerance/recovery, automatic re-balancing.
  - check functions about region split/merge/change-peer.

<!--
A brief description on how the implementation will be tested. Both integration test and unit test should consider the following things:
- How to ensure that the implementation works as expected?
- How will we know nothing broke?
-->

## Build
```
# install before compiling: grpc, c++7, protobuf, python3, golang

# export PROMETHEUS_METRIC_NAME_PREFIX=xxx # prefix for each metrics extended from TiKV. Default is "test_engine_proxy_".
# export ENGINE_LABEL_VALUE=xxx # "engine":"xxx" will be added to store. Default is "test_engine".
# export GRAFANA_DEPLOY_TAG=xxx # Generate grafana dashboard template file. Default is empty.

make

cd demo/bin/

export LD_LIBRARY_PATH=`pwd` ./RaftStoreDemo -V

# version info will be printed. 

``` 

## Contact

[tongzhigao@pingcap.com](mailto:tongzhigao@pingcap.com)

## License

Apache 2.0 license. See the [LICENSE](./LICENSE) file for details.

## Acknowledgments

- Thanks [tikv](https://github.com/tikv/tikv) for providing source code.
- Thanks [pd](https://github.com/tikv/pd) for providing `placement rules`.
