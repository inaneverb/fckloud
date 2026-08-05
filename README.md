# fckloud

# Providers

- :key: https://www.ipify.org/
- :key: https://seeip.org/
- :key: https://www.myip.com/
- :key: https://myip.wtf/
- https://www.bigdatacloud.com/free-api/public-ip-address-api
- https://www.myip.la/
- :zzz: https://httpbin.org/

Providers with :key: mark has their trust factor 2 and more (medium and higher).
Providers with :zzz: mark are disabled by default,
and take part only when named by `--providers`.

`PROVIDERS.md` holds the rest: what each one limits,
what it has been observed to do, and why it carries the trust factor it does.

# Trust factor and Confirmation threshold

Each implemented Provider has its own trust factor, 
which is pre-defined<sup>1</sup> and can be overridden by the user. 
When a provider reports a new IP address, 
that IP must reach a certain threshold to be considered confirmed.

During this process, the IP is added to the pool, 
and when any provider reports the same IP address, 
that provider's trust factor is added to the reported IP's confirmation bucket, 
which accumulates until it reaches the confirmation threshold.

You can define the confirmation threshold, but by default it is:

$$
  C = \begin{cases}
    P_1 & \quad \text{if } n = 1 \\
    \left\lceil \frac{2}{3} \sum_{k=1}^n P_k \right\rceil & \quad \text{if } n = 2 \\
    \left\lfloor \frac{2}{3} \sum_{k=1}^n P_k \right\rfloor & \quad \text{if } n \geq 3
  \end{cases}, \quad
  P_k \in [1..3]
$$

where $C$ is the confirmation threshold and $P_k$ is the Provider's trust factor. 
Two thirds is rounded up while only two providers are enabled, 
and rounded down once there are three or more, 
so that the threshold stays reachable without demanding unanimity. 
The arithmetic is exact: two thirds, not an approximation of it.

As mentioned above, each IP accumulates its own confirmation bucket:

$$
  C_i^{'} = \sum_{k=1}^n P_k \times \delta_{k,i}
$$

where $C_i^{'}$ is the i-th IP's confirmation bucket, 
and $\delta_{k,i}$ indicates whether provider $k$ has reported the i-th IP or not<sup>2</sup> (either 1 or 0). 
The i-th IP is confirmed once $C_i^{'} \geq C$.

With the six providers enabled by default — 
`Ipify` at trust factor $3$, 
`SeeIp`, `MyIpCom` and `MyIpWtf` at $2$, 
`BigDataCloud` and `MyIpLa` at $1$ — 
the total trust is $11$, so $C = \left\lfloor \frac{22}{3} \right\rfloor = 7$. 
No provider therefore confirms an address alone, 
and $4$ trust may go missing before a round loses one it would have confirmed.

The value of $P_k$ must be within the range $[1..3]$, where:
- $1$: Lowest trust; typically assigned to a few providers. 
  This is the default value for new, unknown, or untested providers.
- $2$: Standard (medium) trust; most providers have this value by default.
- $3$: Highest trust; providers with this value can shift consensus significantly. 
  Use with caution and only for providers you fully trust. 
  Only a very few providers have this value by default.

Since you can redefine trust factors for providers, here are important considerations:

- You cannot set a trust factor below 1. 
  Disable the provider entirely if you do not wish to trust it at all.
- You cannot set a trust factor above 3. 
  Disable other providers or assign them the lowest trust factor if you want to establish a master Provider.

:warning: You can specify your own confirmation threshold; 
however, it is highly recommended to use the default value. 
The default threshold is calculated based on enabled providers 
(those participating in consensus) 
and prevents your node from being assigned falsely reported IP addresses. 
Setting this value manually could potentially lead to 
an inability to reach consensus for a single IP (if the threshold is too high) 
or result in falsely reported IPs being assigned to the node (if the threshold is too low).

<sub>
    (1) - The source of pre-defined trust factors is based on my knowledge 
    of the corresponding provider's usage prevalence. 
    This should not be taken as an ultimate truth, 
    since they are simply values that needed to be defined.<br/>
    (2) - The actual code implementation does not use this exact formula. 
    This is merely a demonstration of the underlying concept.
</sub>

# Changelog

### v1.7.0
- Added `--providers`, which names the providers to ask and replaces both `--enable` and `--disable`
- `--enable` and `--disable` are deprecated and hidden from `--help`; they still work, warn when used, and refuse to run beside `--providers`
- Provider names are matched whatever their case, on the flags and in the environment alike
- The resolved pool, every provider's trust factor and rate limit, and the confirmation threshold are logged at startup

### v1.6.0
- Every provider carries the rate limit it publishes, and a round skips one whose gap has not elapsed rather than going over it
- Added `--rate-limit`, which changes the gap a provider asks for, or lifts it when set to zero
- Added `--ignore-rate-limits`, which asks every provider every round whatever it publishes
- Added a counter for the rounds that skipped a rate limited provider
- Removed the "interval could be too short" warning; the gaps are now honoured whatever `--interval` is set to

### v1.5.0
:warning: The set of providers asked by default has changed. 
A node behind an egress allowlist or a NetworkPolicy 
reaches no consensus until the five new hosts are permitted, 
and a manually pinned `--confirmations` now sits against a total trust of 11 rather than 3.

- Added five providers: `Ipify`, `SeeIp`, `MyIpCom`, `BigDataCloud` and `MyIpLa`
- `HttpBin` is now disabled by default: it dropped 19% of the cluster's requests over a measured window, and it is the only provider without IPv6
- Consensus now rests on six providers carrying 11 trust, so no single provider can confirm an address by itself
- Added `--enable`, which replaces the default set of providers rather than adding to it
- `--enable`, `--disable` and `--trust-factor` each take a comma-separated list as well as a repeated flag
- Added `PROVIDERS.md`, where the limits, the observed failure rates and the reasoning behind every trust factor live

### v1.4.0
- Added OpenTelemetry tracing over OTLP: one trace per tick, spanning every provider request and both calls to the API server
- Added OpenTelemetry metrics for provider latency and failures, the consensus outcome, and what each round did to the node's addresses
- Added a rolling count of provider failures over the last hour, day and month
- Telemetry is off until an endpoint is configured, is turned off per signal or altogether by the standard `OTEL_*` variables, and never delays or fails a tick
- A provider that cannot be reached is now only an error when its silence could have cost the round an address, and a warning when consensus did not need it
- Provider failures carry a typed kind rather than a formatted string

### v1.3.0
- Collapsed the three crates into one; the consensus and the Node reconciliation are pure functions now, and covered by tests
- Renamed `ndhcp` to `pubip` and `kubem` to `node`
- Replaced the reserved-address tables with `std` predicates, dropping `ipnet`, `smallvec`, `bytes`, `derive_more` and `strum_macros`
- Fixed every built image being stamped as a dirty build
- Fixed `--version` reporting the version of the previous release
- Pinned the working tree to LF line endings on every platform

### v1.2.0
- Fixed the ExternalIP patch being silently dropped by the API server; the node was never actually updated
- Fixed the confirmation threshold arithmetic, which used `0.67` in place of two thirds and so demanded more agreement than documented
- Fixed a total provider outage stripping the node's ExternalIPs when `--strict` is set
- Operator no longer exits the process on a transient Kubernetes API error
- Added SIGTERM handling, so pod deletion no longer waits out the grace period
- Addresses that are not publicly routable are now rejected, whoever reports them
- Provider requests now time out instead of stalling a whole iteration
- Removed an unchecked UTF-8 assumption on provider responses
- Replaced the `Makefile` with `mise` tasks

### v1.1.0
- Implemented feature "Weighting providers" via trust factor and confirmation number
- Re-purposed confirmation number (now trust factor bucket rather than just a number of providers)
- Added parameter `--trust-factor`
- All IPs during first run are now considered as new, even if they were attached already

### v1.0.0
- Initial release
- Added provider `HttpBin`