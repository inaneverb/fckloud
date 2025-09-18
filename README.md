# fckloud

# Providers

- https://httpbin.org/
- :key: https://myip.wtf/

Providers with :key: mark has their trust factor 2 and more (medium and higher).

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
    \frac{2}{3} \sum_{k=1}^n P_k & \quad \text{if } n > 1 \\
    P_k & \quad \text {if } n = 1
  \end{cases}, \quad
  P_k \in [1..3]
$$

where $C$ is the confirmation threshold and $P_k$ is the Provider's trust factor. 
As mentioned above, each IP accumulates its own confirmation bucket:

$$
  C_i^{'} = \sum_{k=1}^n P_k \times P_i^{'}
$$

where $C_i^{'}$ is the i-th IP's confirmation bucket, 
and $P_i^{'}$ indicates whether provider $P$ has confirmed the i-th IP or not<sup>2</sup> (either 1 or 0).

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

### v1.1.0
- Implemented feature "Weighting providers" via trust factor and confirmation number
- Re-purposed confirmation number (now trust factor bucket rather than just a number of providers)
- Added parameter `--trust-factor`
- All IPs during first run are now considered as new, even if they were attached already

### v1.0.0
- Initial release
- Added provider `HttpBin`