# Memof

A multi backend memory sampler designed to follow processes in benchmarks, returns a json with the data. Available backends:

* **Docker**: will poll the docker runtime to get memory info about a certain container
* **Process**: Keeps track of a process and all it's children

## Installation

```
cargo install memof
```

## Usage

*basic usage:* `memof start <container-name>`
*example usage:* `memof start my-container`

Will keep monitoring the container until it exists or until it is asked to stop.

```shell
memof stop
```

### Other backends 

In order to make use of other backends just make use of the `--backend` flag. Example:

```shell
memof start --backend process my-process
```

## Features

* Easy script integration, works in the background.
* Periodically checks for memory and records the largest usage.
* For processes, takes into account the existence of multiple processes (and even if they are created or destroyed during the execution).
* For processes, keeps on recording as long as any process with the specified name exists.
* Sampling interval of 200ms.

Tested primarily on Linux, but should work on any platform supported by `sysinfo` (including macOS and Windows).

## Output

Generates a `memory.json` file:

```
{
  "memory_kb": 12345
}
```

* memory_kb: Maximum total memory observed across all matching processes during execution.

If no process matches the name at startup `memof` will exit immediately with `{"memory_kb": 0}`.

## Platform-specific behavior

### Process backend

* **Linux**: Uses PSS (Proportional Set Size) from `/proc/<pid>/smaps_rollup`,
  so shared memory is counted proportionally rather than duplicated.
* **macOS/Windows/*BSD**: Uses RSS from `sysinfo`; shared memory may be
  counted multiple times when summing across processes.


## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
