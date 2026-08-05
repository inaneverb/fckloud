## General

- [ ] Add authors to the `--help` command
- [x] Add git commit hash and build date to the `--version` command
- [ ] Add command `providers` to get the list of known providers and their info
- [x] Add native ENV variables support to configure application
- [x] Implement case insensitivity for disabling providers via `--disable`


## Build

- [x] Support `mise` (`mise.toml`) - https://mise.jdx.dev/
- [ ] ~~Support `make` (`Makefile`)~~ - dropped in v1.2.0
- [ ] ~~Support `just` (`justfile`)~~ - not planned
- [ ] ~~Support `task` (`Taskfile.yml`)~~ - not planned


## Deploy

- [x] Write Dockerfile
- [x] Write Kubernetes all-in-one deployment solution


## Healthcheck

- [ ] Implement health check (when operator is running)
- [ ] Implement readiness check (when operator is ready after starting)


## Telemetry

- [x] Implement OpenTelemetry tracing support
- [x] Implement Metrics support
  - [x] Success metrics per provider
  - [x] Response time of each provider


## Providers

- [ ] Keep adding a provider backward compatible: a new one must not shift the default set, the confirmation threshold, or the egress a running deployment needs
- [x] Add provider https://myip.wtf/ (rate: once in 1m)
- [x] Add provider https://seeip.org/ (rate: no limits???)
- [x] Add provider https://www.myip.com/ (rate: no limits???)
- [x] Add provider https://www.ipify.org/ (rate: no limits???)
- [ ] Add provider https://ifconfig.me/all.json (rate: unknown)
- [ ] Add provider https://ifconfig.co/ (rate: once in 1m)
- [x] Add provider https://www.bigdatacloud.com/free-api/public-ip-address-api (rate: no limits???)
- [ ] Add provider https://freeipapi.com/ (rate: once in 1s)
- [ ] Add provider https://api.ident.me/ (rate: unknown)
- [x] Add provider https://www.myip.la/ (rate: no limits???)
- [ ] Add provider https://myexternalip.com/ (rate: once in 2s)
- [ ] Add provider https://icanhazip.com/ (rate: unknown)
- [ ] Add provider https://checkip.amazonaws.com/ (rate: unknown)


## Features

- [x] Implement "Rate limiters" (see below)
- [x] Implement "Weighting providers" (see below)
- [ ] Implement "Dual-stack" (see below)
- [x] Implement "Named provider sets" (see below)
- [ ] Implement "Threshold over responders" (see below)
- [ ] Implement "Removal grace" (see below)
- [ ] Implement "Persisted pending removals" (see below)
- [ ] Implement "Providers from a ConfigMap" (see below)
- [ ] Implement "Non-HTTP providers" (see below)


## Features description

### Rate limiters

Some providers require limit requests to them to specified frequency.
Respect their limitations and use provider at each particular moment only
if it could be used basing on that rate limit.

Add CLI flag to bypass these rate limits (user should have the full control).

### Weighting providers

According to the gathered information, some providers are more wide used,
some are less. Thus it's expectable those that are older and battle-tested
deserves more trust than the others.

Introduce and implement bucketed accumulated value Q, one per each obtained IP.
Each provider has it's own trust factor (K). When provider reports some IP,
its K adds to that IP's Q. 
When Q reaches some threshold, let's say Q', it's assumed confirmed and ready.
  
### Dual-stack

Implement support of both V4/V6 (with predictable and configurable options) stacks.
Also implement multiple-nic support, when user has 2+ public IPv4/IPv6 NIC,
thus having more than 1 public IP in one network stack.

### Named provider sets

One `--providers` replaces `--enable` and `--disable`: version and provider
names, unioned, case insensitive, no subtraction.

Versions pin, are exclusive, and resolve down to the release that last changed
the set. Above this binary's version errors, below but unreleased warns.
`all` is every provider including disabled ones, `trust1|2|3` (`low|med|hig`)
the enabled ones at or above that trust, warning when one was skipped,
`default` what the binary ships with.

Tests: a released minor never changes its set, and only `all` or an explicit
name yields a disabled provider.

Old flags hidden and deprecated, both kinds together refuse to start. Log the
resolved pool at startup. Pin `FCKLOUD_PROVIDERS` in `deploy/k8s.yaml`.

### Threshold over responders

Take the threshold from the trust that answered, not from what was configured,
so an unreachable provider cannot block consensus. Floor it at 2, or at the
total when that is less. Weight decides, never counts.

Grade each round by how much of the enabled trust answered it, within the tick.

`--trust-share` takes a fraction or a percent and replaces `--confirmations`,
which pins an absolute that silently re-means itself when the set changes.
Refuse to start when the resolved need falls below the floor or above the total.

### Removal grace

`--removal-grace`, a duration, defaulting to 5m. An unconfirmed address becomes
eligible once it elapses and goes once two well answered rounds have missed it.
Time alone never reaps, and silence never reaps.

Grace below `--interval` is valid: probe out of tick to fill the window, through
the rate limiter.

`--strict` hidden and deprecated, still reaping on the first round, refusing to
run beside `--removal-grace`.

### Persisted pending removals

Pending removals live in memory, so a restart gives a stale address another full
window. Persisting them needs `nodes: patch` on top of today's `nodes/status`.
Decide whether the privilege is worth it.

### Providers from a ConfigMap

Read the provider table from a ConfigMap: host, URI, JSON field, trust factor.
Restart to pick up changes, no hot reload. Lets a provider be added or dropped
without a release.

### Non-HTTP providers

Add STUN (RFC 8489) and direct-to-authoritative DNS providers, trust 1, never
enough alone, preferring `stuns:` and DoT.

Worth it for failure mode diversity, not for fewer egress rules - both need
egress HTTP does not. DNS returns the resolver's address when a resolver is in
the path, wrong but routable, so verify the authority answered. STUN reports the
address of a UDP flow, which can differ from the HTTP one.
