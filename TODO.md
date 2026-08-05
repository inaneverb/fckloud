## General

- [ ] Add authors to the `--help` command
- [x] Add git commit hash and build date to the `--version` command
- [ ] Add command `providers` to get the list of known providers and their info
- [x] Add native ENV variables support to configure application
- [ ] Implement case insensitivity for disabling providers via `--disable`


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

- [ ] Implement "Rate limiters" (see below)
- [x] Implement "Weighting providers" (see below)
- [ ] Implement "Dual-stack" (see below)


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
