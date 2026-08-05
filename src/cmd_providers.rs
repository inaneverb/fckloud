use {
    crate::{
        Executable,
        pubip::{HttpProvider, TrustFactorAuthority},
    },
    anyhow::Result,
    clap::Args as ClapArgs,
    humantime::Duration as DisplayedDuration,
    strum::VariantArray,
};

/// The list of options for the "providers" command.
#[derive(ClapArgs)]
pub struct Args {
    /// Print the providers as JSON instead of prose
    #[arg(long, default_value_t = false)]
    json: bool,
}

impl Executable for Args {
    fn setup(self) -> Result<Self> {
        Ok(self)
    }

    // Everything here is compiled in, so nothing is fetched and nothing can
    // fail. PROVIDERS.md holds the same information at length.
    async fn run(self) -> Result<()> {
        let tfa = TrustFactorAuthority::default();

        if self.json {
            print!("{}", as_json(&tfa));
            return Ok(());
        }

        for provider in <HttpProvider as VariantArray>::VARIANTS {
            describe(*provider, &tfa);
        }

        summarise(&tfa);
        Ok(())
    }
}

fn describe(provider: HttpProvider, tfa: &TrustFactorAuthority) {
    let rate_limit = provider.rate_limit().map_or_else(
        || "none published".to_owned(),
        |gap| DisplayedDuration::from(gap).to_string(),
    );

    println!("{provider}");
    println!("  trust factor  {}", tfa.trust_factor(provider));
    println!(
        "  by default    {}",
        if provider.enabled_by_default() {
            "asked"
        } else {
            "not asked, name it to enable it"
        },
    );
    println!(
        "  families      {}",
        if provider.has_ipv6() {
            "IPv4, IPv6"
        } else {
            "IPv4 only"
        },
    );
    println!("  rate limit    {rate_limit}");
    println!("  endpoint      {}", provider.request_uri());
    println!("  terms         {}", provider.terms());

    for line in wrapped(provider.summary(), 68) {
        println!("  {line}");
    }

    println!();
}

fn summarise(tfa: &TrustFactorAuthority) {
    let enabled: Vec<HttpProvider> = <HttpProvider as VariantArray>::VARIANTS
        .iter()
        .copied()
        .filter(|provider| provider.enabled_by_default())
        .collect();

    let total: usize = enabled.iter().map(|p| tfa.trust_factor(*p)).sum();

    println!(
        "{} of {} providers are asked by default, carrying {total} trust between them.",
        enabled.len(),
        <HttpProvider as VariantArray>::VARIANTS.len(),
    );
    println!(
        "An address confirms at {}, and at less when fewer of them answer.",
        tfa.calc_confirmation_number(&enabled),
    );
}

fn as_json(tfa: &TrustFactorAuthority) -> String {
    let entries: Vec<String> = <HttpProvider as VariantArray>::VARIANTS
        .iter()
        .map(|provider| {
            let rate_limit = provider
                .rate_limit()
                .map_or_else(|| "null".to_owned(), |gap| gap.as_secs().to_string());

            format!(
                concat!(
                    "  {{\n",
                    "    \"name\": {:?},\n",
                    "    \"host\": {:?},\n",
                    "    \"endpoint\": {:?},\n",
                    "    \"terms\": {:?},\n",
                    "    \"trust_factor\": {},\n",
                    "    \"enabled_by_default\": {},\n",
                    "    \"ipv6\": {},\n",
                    "    \"rate_limit_seconds\": {},\n",
                    "    \"summary\": {:?}\n",
                    "  }}",
                ),
                format!("{provider:?}"),
                provider.host(),
                provider.request_uri(),
                provider.terms(),
                tfa.trust_factor(*provider),
                provider.enabled_by_default(),
                provider.has_ipv6(),
                rate_limit,
                provider.summary(),
            )
        })
        .collect();

    format!("[\n{}\n]\n", entries.join(",\n"))
}

/// Breaks a summary onto lines no wider than `width`, so the output stays
/// readable in a terminal without pulling in a wrapping crate.
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();

    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }

        if !line.is_empty() {
            line.push(' ');
        }

        line.push_str(word);
    }

    if !line.is_empty() {
        lines.push(line);
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_is_broken_at_the_width_it_was_given() {
        let lines = wrapped("one two three four five", 9);

        assert_eq!(lines, vec!["one two", "three", "four five"]);
        assert!(lines.iter().all(|line| line.len() <= 9));
    }

    #[test]
    fn a_word_longer_than_the_width_still_gets_a_line() {
        assert_eq!(
            wrapped("supercalifragilistic", 5),
            vec!["supercalifragilistic"]
        );
    }

    #[test]
    fn every_provider_appears_in_the_json() {
        let json = as_json(&TrustFactorAuthority::default());

        for provider in <HttpProvider as VariantArray>::VARIANTS {
            assert!(json.contains(provider.host()), "{provider} is missing");
        }
    }
}
