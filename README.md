This crate implements async-let as a `#[no_std]`, no-unsafe, no-panic, minimal-overhead library.

Async-let is an experiment based on Conrad Ludgate's musings from <https://conradludgate.com/posts/async-let>,
which proposes a syntax that allows futures to be run "in the background" without spawning additional tasks.
The general behavior of async-let is that the following (fictitious) syntax

```rust
async let metadata = client.request_metadata();
```

will mark a future as a background future. Any await point between the binding and `data.await` will poll
`data` as well as the future being awaited.

```rust
// `metadata` is marked as a background future here
async let metadata = client.request_metadata();

// both `data` and `metadata` are polled while awaiting `data`
let data = client.request_data().await;

// unmark `metadata` as a background future and await it
let metadata = metadata.await;
```

Using this crate's API, the above can be written like this.

```rust
// create a group that will drive the background futures
let group = async_let::Group::new();

// mark `metadata` as a background future by attaching it to the group
let metadata = pin!(client.request_metadata());
let (metadata, group) = group.attach(metadata);

// await `data` using the group, which polls the background futures
let data = group.wait_for(client.request_data()).await;

// detach `metadata` from the group and await it
let (metadata, group) = group.detach_and_wait_for(metadata).await;
```

## API
The main types this crate offers are `Group<List>`, a type that manages manages driving a statically typed
`List` of background futures; and `Handle<Fut>`, an abstract handle that represents the capability to extract
a background future of type `Fut`. The `attach` method adds a future to the list of background futures, the
`detach` method removes a future, and the `wait_for` method produces a future that will poll the background
futures when it is polled.

To minimize the overhead of tracking background futures, each `Group` is associated with a fixed set of futures.
Attaching a future to a group or detaching a future from a group consumes the group and produces a new group with
the future added to or removed from the set of background futures, respectively.

## Limitations
In the quest for minimal overhead, several tradeoffs were made.
- **Ergonomics**: while the API was made to be as ergonomic as possible, callers are still required to manually add and
  remove futures, thread the group through each operation, and locally pin futures explicitly.
- **Drop order**: futures that have been attached to groups will be dropped when their lifetime ends, and *not* when they
  are polled to completion. Even if a future is later detached and awaited, it will not be dropped until its storage
  is deallocated or goes out of scope.
- **Lifetimes**: a group is necessarily moved when a future is attached or detached. If a future is attached in a nested
  scope, it must be detached before leaving that scope, or else the group (and all its attached futures) will be made
  inaccessible.
- **Branching**: because the set of futures is statically tracked, it is not possible to attach a future in only one branch
  of a condtional if one wishes the group to remain accessible after the conditional. Futures of different types may
  be attached to the same location in a group by erasing the type of the attached future to `dyn Future<Output = X>`,
  but this has its limitations.
