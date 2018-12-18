# rs-raft
Raft实现Rust版.

----

## 背景
目的是为了学习Rust和[`Raft`](https://raft.github.io/raft.pdf)

core模块:实现细节~~参考~~照抄[pingcap](https://github.com/pingcap)的[raft-rs](https://github.com/pingcap/raft-rs)

[TODO] 未来会自己写存储部分

## 概念

>`Raft` is an algorithm for managing a replicated log.

`Raft`是一种用来管理复制日志的一致性算法.

## 实现

>`Raft` implements consensus by first electing a distinguished leader, then giving the leader complete responsibility for managing the replicated log. The leader accepts log entries from clients, replicates them on other servers, and tells servers when it is safe to apply log entries to their state machines.

`Raft`通过<u>选举</u>一个高贵的<u>领导人</u>,然后给予它<u>全部职责</u>来<u>管理复制日志</u>从而实现一致性. 领导人接收来自客户端的日志,在其他服务器上复制它们,并告诉服务器当它们是安全的情况下将这些日志应用到状态机.

---
`Raft`将共识问题分解为三个相对独立的子问题:

- Leader election: a new leader must be to chosen when an existing leader fails
    - 领导人选举: 当已存在的领导人宕机时新的领导人必须要被选出来
- Log replication: the leader must accept log entries from clients and replicate them across the cluster, forcing the other logs to agree with its own
    - 日志复制: 领导人必须从客户端接收日志然后复制到集群中的其他节点, 并且强制要求其他节点的日志和自己相同
- Safety: the key safety property for *Raft* is the *State Machine* Property : if any server has applied a particular log entry to its *state machine*, then no other server may apply a different command for same log index.
    - 安全性: Raft的安全性关键是状态机安全: 如果任何服务器已将特定日志条目应用于其状态机,那么其他服务器不能在同一日志索引位置应用不同的命令

### Safety

#### Election Safety

选举安全特性

> at most one leader can be elected in a given term.
> 在一个给定的任期号, 最多只有一个领导人会被选举出来

#### Leader Append-Only

领导人只附加原则

> a leader never overwrites or deletes entries in its log; it only appends new entries.
> 领导人永远不会覆盖或删除其日志中的条目; 它只附加新条目.

#### Log Matching

日志匹配原则

> if two logs contain an entry with the same index and term, then the logs are identical in all entries up through the given index.
> 如果两个日志在相同索引位置的日志条目的任期号相同, 那么这两个日志从头到这个索引位置的都是相同的.

#### Leader Completeness

领导人完全特性

> if a log entry is committed in a given term, then that entry will be present in the logs of the leaders for all higher-numbered terms.
> 如果在给定的任期中提交了某条日志条目, 则该日志条目必然出现在所有更高任期编号的领导人的日志中.

#### State Machine Safety

状态机安全特性

> if a server has applied a log entry at a given index to its state machine, no other server will ever apply a different log entry for the same index.
> 如果服务器已将给定索引处的日志条目应用于其状态机, 那么其他服务器不会在同一索引提交不同的日志条目.

## 精简摘要

### Role

#### struct

```rust
pub enum StateRole {
    /// The node is a follower of the leader.
    Follower,
    /// The node could become a leader.
    Candidate,
    /// The node is a leader.
    Leader,
}
```

### State

#### struct

```rust
pub struct State {
    // Persistent state on all servers (Updated on stable storage before responding to RPCs)
    // 持久化在所有服务器上的状态 (在响应RPC之前更新)
    
    /// 1. currentTerm
    ///     latest term server has seen (initialized to 0 on first boot, increases monotonically)
    ///     服务器已知的最后一次任期号 (初始化为0,持续增加)
    pub current_term: u64,
    
    /// 2. votedFor
    ///     candidateId that received vote in current term (or null if none)
    ///     在当前任期内获得该节点投票的候选人id (没有就为null)
    pub voted_for: u64,
    
    /// 3. log[]
    ///     log entries; each entry contains command for state machine, and term when entry was received by leader (first index is 1)
    ///     日志集合; 每个记录包含状态机的命令, 以及领导者收到条目时的任期 (第一个索引为1)
    pub log: Vec![],
    
    /// Volatile state on all servers
    /// 在所有服务器上面容易变的状态
    
    /// 4. commitIndex
    ///     index of highest log entry known to be committed (initialized to 0, increases monotonically)
    ///     已知最大的被提交日志的索引 (初始化为0, 持续增加)
    pub commit_index: u64,
    
    /// 5. lastApplied
    ///     index of highest log entry applied to state machine (initialized to 0, increases monotonically)
    ///     最后应用于状态机的日志索引 (初始化为0, 持续增加)
    pub last_applied: u64,
    
    
    /// Volatile state on leaders: (Reinitialized after election)
    /// 在领导人上面易变的状态 (选举之后重新初始化)
    
    /// 6. nextIndex[]
    ///     for each server, index of the next log entry to send to that server (initialized to leader last log index + 1)
    ///     对于每个服务器,要发送到该服务器的下一个日志条目的索引 (初始化为领导人最后日志索引 + 1)
    pub next_index: Vec![],
    
    /// 7. matchIndex[]
    ///     for each server, index of highest log entry known to be replicated on server (initialized to 0, increases monotonically)
    ///     对于每个服务器, 已经在服务器上复制的最高日志条目的索引 (初始化为0, 持续递增)
    pub match_index: Vec![],
}
```

### AppendEntries RPC

#### struct

```rust
/// Invoked by leader to replicate log entries ; also used as heartbeat
/// 由leader调用来复制日志; 也用作发送心跳包
pub struct AppendEntriesRPC {
    /// 1. term
    ///     leader’s term
    ///     领导人的任期号
    pub term: u64,
    
    /// 2. leaderId
    ///     so follower can redirect clients
    ///     领导人的id,便于跟随者可以重定向客户端
    pub leader_id: u64,
    
    /// 3. prevLogIndex
    ///     index of log entry immediately preceding new ones
    ///     紧接之前的,新的日志条目的索引
    pub prev_log_index: u64,
    
    /// 4. prevLogTerm
    ///     term of prevLogIndex entry
    ///     prevLogIndex日志条目的索引值
    pub prev_log_term: u64,
    
    /// 5. entries[]
    ///     log entries to store (empty for heartbeat; may send more than one for efficiency)
    ///     需要被保存的日志集合 (为空时表示心跳; 为了提升效率可能会发送多个)
    pub entries: Vec![],
    
    /// 6. leaderCommit
    ///     leader’s commitIndex
    ///     领导人的 被提交日志的最大索引
    pub leader_commit: u64,
}

pub struct Result {
    /// 1. term
    ///     currentTerm, for leader to update itself
    ///     当前任期号, 供领导者更新自己
    pub term: u64,
    
    /// 2. success
    ///     true if follower contained entry matching prevLogIndex and prevLogTerm
    ///     如果跟随者包含匹配prevLogIndex和prevLogTerm的日志条目, 则返回true
    pub success: bool,
}
```

#### Receiver implementation

- Reply false if term < currentTerm
    - 如果term < currentTerm, 则返回false
- Reply false if log doesn’t contain an entry at prevLogIndex whose term matches prevLogTerm
    -  如果日志在prevLogIndex位置的条目,它的任期号与prevLogTerm不匹配,则返回false
- If an existing entry conflicts with a new one (same index but different terms), delete the existing entry and all that follow it
    - 如果已有日志条目与新的日志条目冲突 (索引相同但任期号不同), 删除已有日志条目及其后面的所有日志条目
- Append any new entries not already in the log
    - 附加在日志中尚未存在的任何新日志条目
- If `leaderCommit > commitIndex`, set `commitIndex = min(leaderCommit, index of last new entry)`
    - 如果leaderCommit大于commitIndex, 则设置commitIndex等于 (领导人提交日志的最大索引, 新日志条目中最后的一个的索引) 两个中最小的值

### RequestVote RPC

#### struct

```rust
/// Invoked by candidates to gather votes
/// 由候选人调用来收集选票
pub struct RequestVoteRPC {
    /// 1. term
    ///     candidate’s term
    ///     候选人的任期号
    pub term: u64,
    
    /// 2. candidateId
    ///     candidate requesting vote
    ///     候选人的id, 候选人要求投票
    pub candidate_id: u64,
    
    /// 3. lastLogIndex
    ///     index of candidate’s last log entry
    ///     候选人最后一个日志条目的索引
    pub last_log_index: u64,
    
    /// 4. lastLogTerm
    ///     term of candidate’s last log entry
    ///     候选人最后一个日志条目的任期号
    pub last_log_term: u64,
}

pub struct Result {
    /// 1. term
    ///     currentTerm, for candidate to update itself
    ///     当前任期号, 供候选人更新自己
    pub term: u64,
    
    /// 2. voteGranted
    ///     true means candidate received vote
    ///     true表示候选人获得投票
    pub vote_granted: bool,
}
```

#### Receiver implementation

- Reply false if `term < currentTerm`
    - 如果term小于currentTerm, 则返回false
- If votedFor is null or candidateId, and candidate’s log is at least as up-to-date as receiver’s log, grant vote
    - 如果votedFor等于null或candidateId, 并且候选人的日志至少与接收者的日志一样新时, 则投票给它

### Rules for Servers

#### All Servers

- If `commitIndex > lastApplied`: increment lastApplied, apply log[lastApplied] to state machine
    - 如果commitIndex大于lastApplied: 那么将lastApplied + 1, 并将log[lastApplied]应用到状态机
- If RPC request or response contains `term T > currentTerm`: set `currentTerm = T`, convert to follower
    - 如果RPC请求或响应中 任期号`T`大于`currentTerm`: 那么就将currentTerm设置为任期号T, 并转换为follower

#### Followers

- Respond to RPCs from candidates and leaders
    - 响应候选人和领导人的RPC请求
- If election timeout elapses without receiving AppendEntries RPC from current leader or granting vote to candidate: convert to candidate
    - 如果在选举超时前没有从当前领导者接收到AppendEntries RPC,或者是候选人的RequestVote RPC时: 就将自己转换为候选人

#### Candidates

- On conversion to candidate, start election
    - 转换为候选人后, 立即开始选举
        - Increment currentTerm
            - 自增当前的任期号
        - Vote for self
            - 给自己投票 
        - Reset election timer
            - 重置选举超时计时器 
        - Send RequestVote RPCs to all other servers 
            - 发送RequestVote RPC给其他所有服务器
- If votes received from majority of servers: become leader
    - 如果获得大多数服务器的选票: 那么就变成领导人
- If AppendEntries RPC received from new leader: convert to follower
    - 如果接收到新领导人的AppendEntries RPC: 那么就转变为跟随者
- If election timeout elapses: start new election
    - 如果选举超时: 再发起一轮新选举

#### Leaders

- Upon election: send initial empty AppendEntries RPCs (heartbeat) to each server; repeat during idle periods to prevent election timeouts
    - 领导人当选时: 将空的AppendEntries RPC (心跳) 发送到每个服务器; 在一定空闲期间后不停重复发送以防止选举超时 (阻止跟随者没有收到心跳时,将自己变为候选人)
- If command received from client: append entry to local log, respond after entry applied to state machine
    - 如果从客户端收到请求: 将日志条目附加到本地日志, 在日志条目被应用于状态机后响应客户端
- If last log `index ≥ nextIndex` for a follower: send AppendEntries RPC with log entries starting at nextIndex
    - 对于某个跟随者, 如果<u>最后面的日志索引</u>的大于等于`nextIndex`: 那么发送AppendEntries RPC 从nextIndex开始的所有日志条目
        - If successful: update nextIndex and matchIndex for follower
            - 如果成功: 更新相应跟随者的nextIndex和matchIndex 
        - If AppendEntries fails because of log inconsistency: decrement nextIndex and retry
            - 如果因为日志不一致而失败: 递减nextIndex并重试
- If there exists an N such that `N > commitIndex`, a majority of `matchIndex[i] ≥ N`, and `log[N].term == currentTerm`: set `commitIndex = N`
    - 如果存在一个满足N > commitIndex的N, 并且大多数matchIndex[i] >= N成立, 并且log[N].term == currentTerm成立: 那么就设置commitIndex等于这个N
