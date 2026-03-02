use crate::probe::types::{ProbeMethod, ProbeTarget};

pub fn all_targets() -> Vec<ProbeTarget> {
    vec![
        // ── AI ───────────────────────────────────────────────────────────────
        ProbeTarget { name: "ChatGPT",    category: "ai",  method: ProbeMethod::Trace, url: "https://chatgpt.com/cdn-cgi/trace",          header_key: None },
        ProbeTarget { name: "OpenAI",     category: "ai",  method: ProbeMethod::Trace, url: "https://openai.com/cdn-cgi/trace",            header_key: None },
        ProbeTarget { name: "Claude",     category: "ai",  method: ProbeMethod::Trace, url: "https://claude.ai/cdn-cgi/trace",             header_key: None },
        ProbeTarget { name: "Gemini",     category: "ai",  method: ProbeMethod::Http,  url: "https://gemini.google.com/robots.txt",        header_key: None },
        ProbeTarget { name: "Grok",       category: "ai",  method: ProbeMethod::Trace, url: "https://grok.com/cdn-cgi/trace",              header_key: None },
        ProbeTarget { name: "Perplexity", category: "ai",  method: ProbeMethod::Trace, url: "https://www.perplexity.ai/cdn-cgi/trace",     header_key: None },
        ProbeTarget { name: "Copilot",    category: "ai",  method: ProbeMethod::Http,  url: "https://copilot.microsoft.com/robots.txt",    header_key: None },
        ProbeTarget { name: "Midjourney", category: "ai",  method: ProbeMethod::Trace, url: "https://www.midjourney.com/cdn-cgi/trace",    header_key: None },

        // ── Social ───────────────────────────────────────────────────────────
        ProbeTarget { name: "Twitter",    category: "social", method: ProbeMethod::Trace, url: "https://help.x.com/cdn-cgi/trace",              header_key: None },
        ProbeTarget { name: "Instagram",  category: "social", method: ProbeMethod::Http,  url: "https://www.instagram.com/robots.txt",           header_key: None },
        ProbeTarget { name: "Facebook",   category: "social", method: ProbeMethod::Http,  url: "https://www.facebook.com/robots.txt",            header_key: None },
        ProbeTarget { name: "Threads",    category: "social", method: ProbeMethod::Http,  url: "https://www.threads.net/robots.txt",             header_key: None },
        ProbeTarget { name: "Reddit",     category: "social", method: ProbeMethod::Trace, url: "https://www.reddit.com/cdn-cgi/trace",           header_key: None },
        ProbeTarget { name: "Discord",    category: "social", method: ProbeMethod::Trace, url: "https://gateway.discord.gg/cdn-cgi/trace",       header_key: None },
        ProbeTarget { name: "Telegram",   category: "social", method: ProbeMethod::Http,  url: "https://telegram.org/robots.txt",                header_key: None },
        ProbeTarget { name: "Medium",     category: "social", method: ProbeMethod::Trace, url: "https://medium.com/cdn-cgi/trace",               header_key: None },
        ProbeTarget { name: "Quora",      category: "social", method: ProbeMethod::Trace, url: "https://www.quora.com/cdn-cgi/trace",            header_key: None },
        ProbeTarget { name: "LinkedIn",   category: "social", method: ProbeMethod::Http,  url: "https://www.linkedin.com/robots.txt",            header_key: None },
        ProbeTarget { name: "XiaoHongShu", category: "social", method: ProbeMethod::Header, url: "https://edith.xiaohongshu.com/speedtest",       header_key: Some("xhs-real-ip") },

        // ── Streaming ────────────────────────────────────────────────────────
        ProbeTarget { name: "YouTube",     category: "streaming", method: ProbeMethod::Http,  url: "https://www.youtube.com/robots.txt",             header_key: None },
        ProbeTarget { name: "Netflix",     category: "streaming", method: ProbeMethod::Http,  url: "https://www.netflix.com/robots.txt",             header_key: None },
        ProbeTarget { name: "Spotify",     category: "streaming", method: ProbeMethod::Trace, url: "https://open.spotify.com/cdn-cgi/trace",         header_key: None },
        ProbeTarget { name: "Twitch",      category: "streaming", method: ProbeMethod::Trace, url: "https://www.twitch.tv/cdn-cgi/trace",            header_key: None },
        ProbeTarget { name: "Crunchyroll", category: "streaming", method: ProbeMethod::Trace, url: "https://crunchyroll.com/cdn-cgi/trace",          header_key: None },
        ProbeTarget { name: "AbemaTV",     category: "streaming", method: ProbeMethod::Trace, url: "https://abema.tv/cdn-cgi/trace",                 header_key: None },
        ProbeTarget { name: "TikTok",      category: "streaming", method: ProbeMethod::Http,  url: "https://tiktok.com/robots.txt",                  header_key: None },
        ProbeTarget { name: "Disney+",     category: "streaming", method: ProbeMethod::Http,  url: "https://www.disneyplus.com/robots.txt",          header_key: None },

        // ── Search ───────────────────────────────────────────────────────────
        ProbeTarget { name: "Google",     category: "search", method: ProbeMethod::Http,  url: "https://www.google.com/robots.txt",          header_key: None },
        ProbeTarget { name: "Bing",       category: "search", method: ProbeMethod::Http,  url: "https://www.bing.com/robots.txt",            header_key: None },
        ProbeTarget { name: "DuckDuckGo", category: "search", method: ProbeMethod::Trace, url: "https://duckduckgo.com/cdn-cgi/trace",       header_key: None },
        ProbeTarget { name: "Brave",      category: "search", method: ProbeMethod::Trace, url: "https://search.brave.com/cdn-cgi/trace",     header_key: None },

        // ── News ─────────────────────────────────────────────────────────────
        ProbeTarget { name: "Wikipedia",  category: "news", method: ProbeMethod::Http,  url: "https://www.wikipedia.org/robots.txt",        header_key: None },
        ProbeTarget { name: "BBC",        category: "news", method: ProbeMethod::Http,  url: "https://www.bbc.com/robots.txt",              header_key: None },
        ProbeTarget { name: "Reuters",    category: "news", method: ProbeMethod::Trace, url: "https://www.reuters.com/cdn-cgi/trace",       header_key: None },
        ProbeTarget { name: "NYT",        category: "news", method: ProbeMethod::Http,  url: "https://www.nytimes.com/robots.txt",          header_key: None },

        // ── Game ─────────────────────────────────────────────────────────────
        ProbeTarget { name: "Steam",      category: "game", method: ProbeMethod::Http,  url: "https://store.steampowered.com/robots.txt",   header_key: None },
        ProbeTarget { name: "Epic",       category: "game", method: ProbeMethod::Trace, url: "https://www.epicgames.com/cdn-cgi/trace",     header_key: None },
        ProbeTarget { name: "Battle.net", category: "game", method: ProbeMethod::Http,  url: "https://battle.net/robots.txt",               header_key: None },
        ProbeTarget { name: "PlayStation", category: "game", method: ProbeMethod::Http, url: "https://www.playstation.com/robots.txt",      header_key: None },
        ProbeTarget { name: "Xbox",       category: "game", method: ProbeMethod::Http,  url: "https://www.xbox.com/robots.txt",             header_key: None },

        // ── Dev ──────────────────────────────────────────────────────────────
        ProbeTarget { name: "GitHub",     category: "dev", method: ProbeMethod::Http,  url: "https://github.com/robots.txt",               header_key: None },
        ProbeTarget { name: "GitLab",     category: "dev", method: ProbeMethod::Trace, url: "https://gitlab.com/cdn-cgi/trace",            header_key: None },
        ProbeTarget { name: "Cloudflare", category: "dev", method: ProbeMethod::Trace, url: "https://www.cloudflare.com/cdn-cgi/trace",    header_key: None },
        ProbeTarget { name: "NPM",        category: "dev", method: ProbeMethod::Trace, url: "https://registry.npmjs.org/cdn-cgi/trace",    header_key: None },
        ProbeTarget { name: "Docker Hub", category: "dev", method: ProbeMethod::Http,  url: "https://hub.docker.com/robots.txt",           header_key: None },
        ProbeTarget { name: "PyPI",       category: "dev", method: ProbeMethod::Http,  url: "https://pypi.org/robots.txt",                 header_key: None },

        // ── Cloud ────────────────────────────────────────────────────────────
        ProbeTarget { name: "AWS",        category: "cloud", method: ProbeMethod::Http, url: "https://aws.amazon.com/robots.txt",           header_key: None },
        ProbeTarget { name: "GCP",        category: "cloud", method: ProbeMethod::Http, url: "https://cloud.google.com/robots.txt",         header_key: None },
        ProbeTarget { name: "Azure",      category: "cloud", method: ProbeMethod::Http, url: "https://azure.microsoft.com/robots.txt",      header_key: None },
        ProbeTarget { name: "Vercel",     category: "cloud", method: ProbeMethod::Http, url: "https://vercel.com/robots.txt",               header_key: None },
        ProbeTarget { name: "Render",     category: "cloud", method: ProbeMethod::Trace, url: "https://render.com/cdn-cgi/trace",           header_key: None },

        // ── Crypto ───────────────────────────────────────────────────────────
        ProbeTarget { name: "Binance",    category: "crypto", method: ProbeMethod::Http,  url: "https://www.binance.com/robots.txt",        header_key: None },
        ProbeTarget { name: "Coinbase",   category: "crypto", method: ProbeMethod::Trace, url: "https://coinbase.com/cdn-cgi/trace",        header_key: None },
        ProbeTarget { name: "OKX",        category: "crypto", method: ProbeMethod::Trace, url: "https://www.okx.com/cdn-cgi/trace",         header_key: None },
        ProbeTarget { name: "Bybit",      category: "crypto", method: ProbeMethod::Http,  url: "https://www.bybit.com/robots.txt",          header_key: None },
        ProbeTarget { name: "Gate.io",    category: "crypto", method: ProbeMethod::Trace, url: "https://www.gate.io/cdn-cgi/trace",         header_key: None },

        // ── NSFW ─────────────────────────────────────────────────────────────
        ProbeTarget { name: "E-Hentai",  category: "nsfw", method: ProbeMethod::Trace, url: "https://e-hentai.org/cdn-cgi/trace",          header_key: None },
        ProbeTarget { name: "MissAV",    category: "nsfw", method: ProbeMethod::Trace, url: "https://missav.ws/cdn-cgi/trace",             header_key: None },
        ProbeTarget { name: "JAVDB",     category: "nsfw", method: ProbeMethod::Trace, url: "https://javdb.com/cdn-cgi/trace",             header_key: None },
        ProbeTarget { name: "Hanime1",   category: "nsfw", method: ProbeMethod::Trace, url: "https://hanime1.me/cdn-cgi/trace",            header_key: None },
        ProbeTarget { name: "91Porn",    category: "nsfw", method: ProbeMethod::Trace, url: "https://91porn.com/cdn-cgi/trace",            header_key: None },
        ProbeTarget { name: "Haijiao",   category: "nsfw", method: ProbeMethod::Trace, url: "https://haijiao.com/cdn-cgi/trace",           header_key: None },

        // ── CN ───────────────────────────────────────────────────────────────
        ProbeTarget { name: "QQ",        category: "cn", method: ProbeMethod::Http,      url: "https://www.qq.com/robots.txt",              header_key: None },
        ProbeTarget { name: "WeChat",    category: "cn", method: ProbeMethod::Http,      url: "https://weixin.qq.com/robots.txt",           header_key: None },
        ProbeTarget { name: "Bilibili",  category: "cn", method: ProbeMethod::Http,      url: "https://www.bilibili.com/robots.txt",        header_key: None },
        ProbeTarget { name: "Weibo",     category: "cn", method: ProbeMethod::Http,      url: "https://weibo.com/robots.txt",               header_key: None },
        ProbeTarget { name: "Baidu",     category: "cn", method: ProbeMethod::Http,      url: "https://www.baidu.com/robots.txt",           header_key: None },
        ProbeTarget { name: "Taobao",    category: "cn", method: ProbeMethod::Http,      url: "https://www.taobao.com/robots.txt",          header_key: None },
        ProbeTarget { name: "JD",        category: "cn", method: ProbeMethod::Http,      url: "https://www.jd.com/robots.txt",              header_key: None },
        ProbeTarget { name: "Xiaomi",    category: "cn", method: ProbeMethod::Http,      url: "https://www.mi.com/robots.txt",              header_key: None },
        ProbeTarget { name: "IPIP",      category: "cn", method: ProbeMethod::ApiDirect, url: "https://myip.ipip.net/json",                 header_key: None },
        ProbeTarget { name: "ByteDance", category: "cn", method: ProbeMethod::Header,    url: "https://perfops.byte-test.com/500b-bench.jpg", header_key: Some("x-request-ip") },
        ProbeTarget { name: "NetEase",   category: "cn", method: ProbeMethod::Header,    url: "https://necaptcha.nosdn.127.net/ab7f4275c1744aa28e0a8f3a1c58c532.png", header_key: Some("cdn-user-ip") },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN_CATEGORIES: &[&str] = &[
        "ai", "social", "streaming", "search", "news",
        "game", "dev", "cloud", "crypto", "nsfw", "cn",
    ];

    #[test]
    fn all_targets_not_empty() {
        assert!(!all_targets().is_empty());
    }

    #[test]
    fn all_target_names_nonempty() {
        for t in all_targets() {
            assert!(!t.name.is_empty(), "target has empty name");
        }
    }

    #[test]
    fn all_target_urls_valid() {
        for t in all_targets() {
            assert!(
                t.url.starts_with("https://"),
                "target '{}' URL does not start with https://: {}",
                t.name, t.url
            );
        }
    }

    #[test]
    fn all_target_categories_known() {
        for t in all_targets() {
            assert!(
                KNOWN_CATEGORIES.contains(&t.category),
                "target '{}' has unknown category '{}'",
                t.name, t.category
            );
        }
    }

    #[test]
    fn header_method_requires_header_key() {
        for t in all_targets() {
            if t.method == ProbeMethod::Header {
                assert!(
                    t.header_key.is_some(),
                    "target '{}' uses Header method but has no header_key",
                    t.name
                );
            }
        }
    }

    #[test]
    fn no_chinese_in_site_names() {
        for t in all_targets() {
            let has_cjk = t.name.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
            assert!(!has_cjk, "target name contains CJK characters: '{}'", t.name);
        }
    }

    #[test]
    fn target_names_no_duplicates() {
        let targets = all_targets();
        let mut names = std::collections::HashSet::new();
        for t in &targets {
            assert!(names.insert(t.name), "duplicate target name: '{}'", t.name);
        }
    }
}
