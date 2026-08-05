# Providers

Where each provider's trust factor comes from, and what it costs to ask it.
`README.md` has the arithmetic; this file has the evidence.

| Provider | Endpoint | Trust | Default | Families |
|---|---|---|---|---|
| `Ipify` | `https://api64.ipify.org/?format=json` | 3 | on | v4 + v6 |
| `SeeIp` | `https://api.seeip.org/jsonip` | 2 | on | v4 + v6 |
| `MyIpCom` | `https://api.myip.com/` | 2 | on | v4 + v6 |
| `MyIpWtf` | `https://myip.wtf/json` | 2 | on | v4 + v6 |
| `BigDataCloud` | `https://api.bigdatacloud.net/data/client-ip` | 1 | on | v4 + v6 |
| `MyIpLa` | `https://api.myip.la/en?json` | 1 | on | v4 + v6 |
| `HttpBin` | `https://httpbin.org/ip` | 1 | off | v4 |

The six on by default carry 11 trust and confirm at 7.

## What a rank is worth

- **3** — auditable. Named operator, public source, a history long enough to
  have one. Rare on purpose.
- **2** — a public-IP service that states its limit and has somebody behind it.
- **1** — a contract knowable only by observation, or one that has been
  measured failing.

Under each entry, `Seen` is a dated log. Only observations that changed a
decision go in it, so most entries have none. When the log stops matching the
rank, the rank moves.

## Ipify — 3

`{"ip":"<public-ip>"}` · Apache-2.0, source at
<https://github.com/rdegges/ipify-api> · terms at <https://www.ipify.org>

No stated limit of any kind, and no visitor logging.

**`api64`, not `api`.** `api.ipify.org` publishes no AAAA. It would answer IPv4
while dual-stacked providers answer IPv6, and a round split across two families
confirms neither address. `api6` is the IPv6-only host if it is ever wanted.

## SeeIp — 2

`{"ip":"<public-ip>"}` · terms at <https://seeip.org>

Funded by UNVIO, LLC, open source, documented as usable "without any real
limit", no visitor logging. `ipv4.seeip.org` and `ipv6.seeip.org` pin a single
family.

Less public history than ipify, and that alone is the gap between 2 and 3.

## MyIpCom — 2

`{"ip":"<public-ip>","country":"Serbia","cc":"RS"}` · terms at
<https://www.myip.com/api-docs/>

"There is no request limit, the only restriction is the server capacity."
Commercial use allowed, credit appreciated.

Answers `text/html` whatever the body actually is, so nothing may key off the
content type. The docs pages refuse a command-line fetch with 403 while the API
host does not — read them in a browser.

## MyIpWtf — 2

`{"YourFuckingIPAddress":"<public-ip>", ...}` · terms at
<https://myip.wtf/automation>

**One request per minute per machine**, which is exactly the default tick and
leaves no headroom. The limit is asked for rather than enforced — a burst of
eight in a few seconds drew no 429 — and is to be honoured anyway. Dropping
`--interval` below `1m` puts this provider over its stated limit before any
other.

## BigDataCloud — 1

`{"ipString":"<public-ip>","ipType":"IPv4"}` · terms at
<https://www.bigdatacloud.com/terms-and-conditions>

No API key, no published limit, a company's infrastructure behind it, and the
fastest answer of the seven. All of that argues for 2.

The documentation argues against it and wins: the published example names the
field `ip`, and the endpoint has only ever sent `ipString`. The page has been
wrong long enough for nobody to notice, so it will not warn about the next
change either, and a contract knowable only by observation is a rank 1
contract. A test pins the mismatch. `api-bdc.net` is the company's mirror and
serves the same body.

## MyIpLa — 1

`{"ip":"<public-ip>","location":{...}}` · terms at <https://www.myip.la>

Documented as unlimited, no visitor logging, plain text at `https://api.myip.la`.

No named operator, no source, no terms page, no contact. A tiebreaker at the
smallest weight there is.

## HttpBin — 1, off by default

`{"origin":"<public-ip>"}` · no terms; the public instance is a courtesy ·
source at <https://github.com/postmanlabs/httpbin>

Publishes no AAAA, so it cannot be reached from an IPv6-only node and splits
the round on a dual-stacked one.

Worth turning on against a local instance, where neither the failure rate below
nor the missing AAAA applies. `--enable` replaces the default set rather than
adding to it, so name everything wanted:

```
fckloud run --node NAME --enable HttpBin,Ipify,SeeIp
```

`Seen`

- 2026-08-05 — 145 failures in 711 requests (20.4%) across three nodes over
  four hours, every one of them `connection closed before message completed`.
  Sample small enough and it says nothing: eight consecutive requests from a
  node succeeded inside that same window.

## Adding one

Four `match` arms in `src/pubip/provider.rs`, a trust factor in
`src/pubip/trust.rs`, a captured body in the `SHAPES` table beside the decoder,
and an entry here. Check for an AAAA record while you are at it — a v4-only
provider among dual-stacked ones fails as consensus quietly not happening.
