# Architecture

HiveBox creates isolated execution environments using Linux kernel primitives — no containers, no VMs, just direct kernel features.

## Security Layers

Each sandbox is protected by six independent security layers:

```
┌─────────────────────────────────────────────────┐
│                  User Command                    │
├─────────────────────────────────────────────────┤
│  6. seccomp-BPF     — syscall allow-list        │
│  5. Capabilities    — minimal privilege set      │
│  4. Landlock        — filesystem path control    │
│  3. pivot_root      — filesystem isolation       │
│  2. cgroup v2       — resource limits            │
│  1. Namespaces      — process/net/mount/user     │
├─────────────────────────────────────────────────┤
│                  Linux Kernel                     │
└─────────────────────────────────────────────────┘
```

### 1. Namespaces (isolation)

Each sandbox gets six namespaces:

| Namespace | Flag | Effect |
|-----------|------|--------|
| PID | `CLONE_NEWPID` | Sandbox sees only its own processes |
| Mount | `CLONE_NEWNS` | Sandbox has its own mount table |
| Network | `CLONE_NEWNET` | Sandbox gets an empty network stack |
| User | `CLONE_NEWUSER` | UID 0 inside maps to unprivileged user outside |
| UTS | `CLONE_NEWUTS` | Sandbox has its own hostname |
| IPC | `CLONE_NEWIPC` | Isolated System V IPC / POSIX queues |

### 2. cgroup v2 (resource limits)

Every sandbox is placed in a dedicated cgroup with configurable limits:

- **Memory**: hard limit with swap disabled
- **CPU**: fractional CPU allocation
- **PIDs**: prevents fork bombs

### 3. pivot_root (filesystem)

After mounting the sandbox rootfs, `pivot_root` swaps the filesystem root. The host filesystem becomes completely unreachable — there is no "above" to escape to.

### 4. Landlock (path restrictions)

Landlock LSM restricts which paths the sandbox can access, even for allowed syscalls. After `pivot_root`, only the sandbox's own root is accessible.

### 5. Capabilities (privilege dropping)

All Linux capabilities are dropped except the minimal set needed:
- `CAP_SETUID`, `CAP_SETGID` — for user switching
- `CAP_FOWNER`, `CAP_DAC_OVERRIDE` — for file access

`PR_SET_NO_NEW_PRIVS` prevents privilege escalation through `execve`.

### 6. seccomp-BPF (syscall filtering)

The final layer installs a BPF filter that allows only whitelisted syscalls:
- **Default profile**: ~80 syscalls for general workloads
- **Strict profile**: ~40 syscalls for pure computation

Any unlisted syscall returns `EPERM`.

## Filesystem Layout

```
/var/lib/hivebox/
├── images/
│   └── base.squashfs        # Debian minirootfs (read-only)
├── sandboxes/
│   └── hb-7f3a9b/
│       ├── squashfs/         # squashfs mount (read-only lower)
│       ├── upper/            # tmpfs (writable upper)
│       ├── work/             # overlayfs workdir
│       └── merged/           # overlayfs union → sandbox root
└── network/
    └── ip_state.json         # IP allocation tracker
```

### Overlayfs Stack

```
                 ┌──────────────────────┐
Sandbox sees:    │    merged/ (rw)      │  ← overlayfs union
                 ├──────────────────────┤
Writes go to:    │    upper/ (tmpfs)    │  ← vanishes on destroy
                 ├──────────────────────┤
Reads from:      │  squashfs/ (ro)      │  ← shared across sandboxes
                 └──────────────────────┘
```

## Networking

Three network modes:

### None (default)
No network interface (not even loopback in strict mode). Maximum isolation.

### Isolated
veth pair connecting the sandbox to a bridge with NAT to the internet:

```
Host                          Sandbox
┌───────────────┐           ┌──────────────────┐
│ veth-{id}     │───────────│ eth0 (10.10.0.x) │
│ (on hivebox0) │           └──────────────────┘
└───────────────┘
       │
  hivebox0 bridge (10.10.0.1)
       │
  iptables MASQUERADE → internet
```

### Shared
Multiple sandboxes on a named bridge can communicate with each other.

## Persistent Sandbox Lifecycle

```
create()                 exec()              destroy()
   │                       │                    │
   ├─ prepare rootfs       ├─ nsenter           ├─ SIGKILL init
   ├─ spawn init process   ├─ /bin/sh -c cmd    ├─ kill cgroup
   ├─ setup cgroup         ├─ capture output    ├─ cleanup network
   ├─ setup network        └─ return result     ├─ unmount overlayfs
   └─ register in manager                       └─ remove directory
```

The init process (`sleep infinity`) keeps namespaces alive between commands. `nsenter` enters the existing namespaces for each `exec`.

## API Architecture

```
Client → axum Router → auth middleware → handler → SandboxManager → Linux kernel
                                                         │
                                                    RwLock<HashMap>
                                                    (thread-safe registry)
```

The `SandboxManager` holds all active sandboxes in a `RwLock<HashMap>`. A background reaper task periodically checks for expired sandboxes and destroys them.
