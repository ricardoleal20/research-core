// Multi-source link resolver (FR-8.1 / FR-15.1, the owner's "no solo arXiv"):
// one pasted link — or a bare DOI / arXiv id — resolves to real paper metadata
// from MANY sources, detected from the input itself:
//
//   arxiv.org/abs|pdf/<id> or a bare arXiv id  → the arXiv export API (the
//                                                 same adapter the v1 flow
//                                                 used, reused verbatim)
//   doi.org/<doi> / dx.doi.org/<doi> / 10.…    → Crossref REST, with an
//                                                 OpenAlex DOI lookup as the
//                                                 fallback when Crossref has
//                                                 nothing
//   api.crossref.org/works/<doi>               → Crossref REST (direct)
//   pubmed.ncbi.nlm.nih.gov/<id>               → PubMed E-utilities esummary
//   semanticscholar.org/paper/<id>             → Semantic Scholar graph API
//   openalex.org/works/<id>                    → OpenAlex
//   any other URL carrying a DOI in its path   → OpenAlex by DOI (best effort)
//   free text (a title, not a URL)             → OpenAlex title search (last
//                                                 resort, best effort)
//
// Everything else is the honest typed `unsupported_source:` refusal listing
// what IS supported — metadata is never fabricated (honesty, EXPERIENCE.md).
// Metadata resolution is a data fetch, not an LLM call (AD-9 governs LLM
// calls only) — no provider is involved.
//
// Both doors (the onboarding first-value paste and the library add-by-link)
// resolve through `resolve_link`, so the two can never drift. Errors are
// coded and bilingual-safe (`unsupported_source:`, `invalid_url:`,
// `resolve_failed:` — codes are never translated).
//
// Tests never touch the network: `resolve_link_with` takes the API base URLs,
// and the test suite points them at a local axum fake server.

use serde::{Deserialize, Serialize};

/// The resolved paper — normalized across sources. `source` is where the
/// paste TARGETED (arxiv | doi | crossref | pubmed | s2 | openalex); `url`
/// and `doi` are the resolved canonical identifiers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedPaper {
    pub source: String,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub venue: String,
    pub doi: Option<String>,
    pub url: String,
    pub abstract_text: Option<String>,
    pub arxiv_id: Option<String>,
}

/// Everything that can go wrong resolving a link — typed, coded,
/// bilingual-safe by construction.
#[derive(Debug, thiserror::Error)]
pub enum ResolverError {
    #[error(
        "unsupported_source: `{0}` — paste a link or id from arXiv, doi.org, PubMed or Semantic \
         Scholar: an arXiv URL or id, a DOI like 10.1038/nature14539, or a journal link carrying \
         its DOI / pega un enlace o id de arXiv, doi.org, PubMed o Semantic Scholar: una URL o \
         id de arXiv, un DOI como 10.1038/nature14539, o un enlace de revista que lleve su DOI"
    )]
    Unsupported(String),
    #[error("invalid_url: `{0}` — paste a valid link (https://arxiv.org/abs/1706.03762)")]
    InvalidUrl(String),
    #[error("resolve_failed: {0}")]
    Failed(String),
}

/// The public API base URLs, injectable so tests run against a local fake
/// server instead of the live endpoints.
#[derive(Debug, Clone)]
pub struct ApiBases {
    pub arxiv: String,
    pub crossref: String,
    pub pubmed: String,
    pub s2: String,
    pub openalex: String,
}

impl Default for ApiBases {
    fn default() -> Self {
        Self {
            arxiv: "https://export.arxiv.org/api".into(),
            crossref: "https://api.crossref.org".into(),
            pubmed: "https://eutils.ncbi.nlm.nih.gov/entrez/eutils".into(),
            s2: "https://api.semanticscholar.org/graph/v1".into(),
            openalex: "https://api.openalex.org".into(),
        }
    }
}

/// Where the input points — the pure detection, testable without any network.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Target {
    Arxiv(String),
    Doi(String),
    Crossref(String),
    Pubmed(String),
    S2(String),
    Openalex(String),
    OpenalexDoi(String),
    OpenalexSearch(String),
}

/// Detect the target source from the pasted input. Pure. arXiv detection
/// reuses the existing `parse_arxiv_url` (arXiv URLs AND bare ids — the v1
/// behavior, kept verbatim); the rest is host- and shape-based.
pub(crate) fn detect_target(input: &str) -> Result<Target, ResolverError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(ResolverError::Unsupported(s.to_string()));
    }
    // arXiv first: the v1 parser accepts arxiv.org URLs, the export host and
    // bare ids — anything it accepts keeps resolving through arXiv.
    if let Ok(arxiv_id) = crate::domain::onboarding::parse_arxiv_url(s) {
        return Ok(Target::Arxiv(arxiv_id));
    }
    // Split the scheme; a non-http(s) scheme is a typed invalid_url (the v1
    // parser's rule, applied to every other host too).
    let (scheme, rest) = match s.split_once("://") {
        Some((scheme, rest)) => {
            if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
                return Err(ResolverError::InvalidUrl(s.to_string()));
            }
            (Some(scheme), rest)
        }
        None => (None, s),
    };
    let lower = rest.to_lowercase();
    let segments = |skip: usize| -> Vec<&str> {
        rest.split('/').filter(|p| !p.is_empty()).skip(skip).collect()
    };
    if lower.starts_with("doi.org/") || lower.starts_with("dx.doi.org/") {
        let path = rest.split_once('/').map(|(_, p)| p).unwrap_or("");
        let doi = path
            .split('?')
            .next()
            .unwrap_or("")
            .trim()
            .trim_end_matches('/')
            .to_string();
        if !looks_like_doi(&doi) {
            return Err(ResolverError::Unsupported(s.to_string()));
        }
        return Ok(Target::Doi(doi));
    }
    if lower.starts_with("api.crossref.org/works/") {
        let doi = segments(2).join("/").trim_end_matches('/').to_string();
        if !looks_like_doi(&doi) {
            return Err(ResolverError::Unsupported(s.to_string()));
        }
        return Ok(Target::Crossref(doi));
    }
    if lower.starts_with("pubmed.ncbi.nlm.nih.gov/") {
        let id = segments(1).join("/").trim_end_matches('/').to_string();
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            return Err(ResolverError::Unsupported(s.to_string()));
        }
        return Ok(Target::Pubmed(id));
    }
    if lower.starts_with("www.ncbi.nlm.nih.gov/pubmed/")
        || lower.starts_with("ncbi.nlm.nih.gov/pubmed/")
    {
        let id = segments(2).join("/").trim_end_matches('/').to_string();
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            return Err(ResolverError::Unsupported(s.to_string()));
        }
        return Ok(Target::Pubmed(id));
    }
    if lower.starts_with("semanticscholar.org/paper/")
        || lower.starts_with("www.semanticscholar.org/paper/")
    {
        // the paper id is the trailing 40-hex-char id after the title slug
        // (`/paper/Attention-Is-All-You-Need-204e30…`)
        let last = segments(2)
            .join("/")
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();
        let id = if last.len() > 40 && last.is_char_boundary(last.len() - 40) {
            let (head, hex) = last.split_at(last.len() - 40);
            if hex.chars().all(|c| c.is_ascii_hexdigit()) && !head.is_empty() {
                hex.to_string()
            } else {
                last
            }
        } else {
            last
        };
        if id.is_empty() {
            return Err(ResolverError::Unsupported(s.to_string()));
        }
        return Ok(Target::S2(id));
    }
    if lower.starts_with("api.semanticscholar.org/graph/v1/paper/") {
        // the explicit API form: the id IS the last path segment
        // (`arXiv:1706.03762`, a 40-hex id, `DOI:…`, `CorpusID:…`)
        let id = segments(4).join("/").trim_end_matches('/').to_string();
        let id = id.rsplit('/').next().unwrap_or("").to_string();
        if id.is_empty() {
            return Err(ResolverError::Unsupported(s.to_string()));
        }
        return Ok(Target::S2(id));
    }
    if lower.starts_with("openalex.org/works/") || lower.starts_with("api.openalex.org/works/") {
        let id = segments(2).join("/").trim_end_matches('/').to_string();
        if id.is_empty() {
            return Err(ResolverError::Unsupported(s.to_string()));
        }
        return Ok(Target::Openalex(id));
    }
    // A bare DOI (no scheme, no host): `10.1038/nature14539` (a trailing
    // sentence period is not part of the DOI).
    if looks_like_doi(rest) {
        return Ok(Target::Doi(rest.trim_end_matches('.').to_string()));
    }
    // A URL we do not recognize: best effort — a journal link usually
    // carries its DOI in the path (`…/doi/10.1111/j.x`, `…/article/10.1007/s…`)
    // or in a query parameter (`…?id=10.1371/journal.pone…`).
    if scheme.is_some() || is_host_like(rest) {
        if let Some(doi) = extract_doi_from_url(rest) {
            return Ok(Target::OpenalexDoi(doi));
        }
        return Err(ResolverError::Unsupported(s.to_string()));
    }
    // Free text (no scheme, no host): a title — the OpenAlex title search is
    // the last resort, best effort.
    if rest.split_whitespace().count() >= 2 {
        return Ok(Target::OpenalexSearch(rest.to_string()));
    }
    Err(ResolverError::Unsupported(s.to_string()))
}

/// `10.1038/nature14539` — the DOI shape: prefix `10.`, a 4-9 digit
/// registrant, a `/`, and a non-empty whitespace-free suffix (the suffix may
/// itself contain `/`, as in `10.1093/brain/awh270`).
fn looks_like_doi(s: &str) -> bool {
    let s = s.trim();
    let Some((prefix, suffix)) = s.split_once('/') else {
        return false;
    };
    let Some(registrant) = prefix.strip_prefix("10.") else {
        return false;
    };
    (4..=9).contains(&registrant.len())
        && registrant.bytes().all(|b| b.is_ascii_digit())
        && !suffix.is_empty()
        && !suffix.chars().any(|c| c.is_whitespace())
}

/// Host-like input: the first `/`-segment carries a dot and no whitespace
/// (`link.springer.com/article/…` is; `attention is all you need. Deep` — a
/// title with a period — is not).
fn is_host_like(s: &str) -> bool {
    let first = s.split('/').next().unwrap_or("");
    first.contains('.') && !first.chars().any(|c| c.is_whitespace())
}

/// The site sections a journal link may append AFTER the DOI — stripped
/// before the path is joined back into the DOI.
const TRAILING_SECTIONS: [&str; 8] = [
    "abstract", "full", "abs", "pdf", "references", "related", "epdf", "html",
];

/// Extract a DOI from an unrecognized URL — the best-effort journal-link
/// fallback: the first path segment matching the DOI prefix shape, joined
/// with the remaining segments (minus trailing site sections), or a query
/// parameter carrying a DOI whole.
fn extract_doi_from_url(url: &str) -> Option<String> {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (url, None),
    };
    if let Some(doi) = extract_doi_from_segments(path) {
        return Some(doi);
    }
    // a query parameter may carry the DOI whole (`?id=10.1371/journal.pone…`)
    if let Some(query) = query {
        for pair in query.split('&') {
            let value = pair.split_once('=').map(|(_, v)| v).unwrap_or(pair);
            if looks_like_doi(value) {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn extract_doi_from_segments(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let start = segments.iter().position(|seg| {
        match seg.split_once('.') {
            Some(("10", registrant)) => {
                (4..=9).contains(&registrant.len())
                    && registrant.bytes().all(|b| b.is_ascii_digit())
            }
            _ => false,
        }
    })?;
    if start + 1 >= segments.len() {
        return None;
    }
    let mut end = segments.len();
    while end > start + 2 && TRAILING_SECTIONS.contains(&segments[end - 1].to_lowercase().as_str())
    {
        end -= 1;
    }
    let doi = segments[start..end].join("/");
    looks_like_doi(&doi).then_some(doi)
}

/// Resolve a pasted link (or bare DOI / arXiv id) into real paper metadata.
/// The production door: both the onboarding paste and the library add call
/// this one function.
pub async fn resolve_link(url: &str) -> Result<ResolvedPaper, ResolverError> {
    resolve_link_with(&ApiBases::default(), url).await
}

/// The testable core: the same resolution against injectable API bases (the
/// test suite points them at a local fake server — no live endpoints).
pub async fn resolve_link_with(
    bases: &ApiBases,
    url: &str,
) -> Result<ResolvedPaper, ResolverError> {
    let target = detect_target(url)?;
    let client = reqwest::Client::builder()
        .user_agent("research-core/0.1")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ResolverError::Failed(format!("client build: {e}")))?;
    match target {
        Target::Arxiv(id) => fetch_arxiv(&client, bases, &id).await,
        Target::Crossref(doi) => {
            fetch_crossref(&client, bases, &doi)
                .await
                .map(|p| ResolvedPaper { source: "crossref".into(), ..p })
        }
        Target::Doi(doi) => {
            // Crossref first; OpenAlex by DOI as the fallback — both honest
            // failures surface, with both reasons named.
            match fetch_crossref(&client, bases, &doi).await {
                Ok(paper) => Ok(ResolvedPaper { source: "doi".into(), ..paper }),
                Err(crossref_err) => {
                    fetch_openalex_by_doi(&client, bases, &doi)
                        .await
                        .map(|p| ResolvedPaper { source: "doi".into(), ..p })
                        .map_err(|fallback_err| {
                            ResolverError::Failed(format!(
                                "no metadata for DOI `{doi}` (Crossref: {crossref_err}; \
                                 OpenAlex: {fallback_err})"
                            ))
                        })
                }
            }
        }
        Target::Pubmed(id) => fetch_pubmed(&client, bases, &id).await,
        Target::S2(id) => fetch_s2(&client, bases, &id).await,
        Target::Openalex(id) => fetch_openalex_work(&client, bases, &id).await,
        Target::OpenalexDoi(doi) => fetch_openalex_by_doi(&client, bases, &doi).await,
        Target::OpenalexSearch(title) => fetch_openalex_search(&client, bases, &title).await,
    }
}

async fn get_json(
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, ResolverError> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| ResolverError::Failed(format!("GET {url}: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(ResolverError::Failed(format!("GET {url} answered HTTP {status}")));
    }
    resp.json().await.map_err(|e| ResolverError::Failed(format!("GET {url}: {e}")))
}

/// The arXiv door (the v1 adapter, reused): the export API's Atom feed.
async fn fetch_arxiv(
    client: &reqwest::Client,
    bases: &ApiBases,
    arxiv_id: &str,
) -> Result<ResolvedPaper, ResolverError> {
    let url = format!("{}/query?id_list={}", bases.arxiv, urlenc(arxiv_id));
    let text = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ResolverError::Failed(format!("GET {url}: {e}")))?
        .text()
        .await
        .map_err(|e| ResolverError::Failed(format!("GET {url}: {e}")))?;
    let search = crate::mcp::parse_arxiv_atom(&text)
        .into_iter()
        .next()
        .ok_or_else(|| ResolverError::Failed(format!("arXiv has no paper with id `{arxiv_id}`")))?;
    Ok(ResolvedPaper {
        source: "arxiv".into(),
        title: search.title,
        authors: search.authors,
        year: search.year,
        venue: search.venue,
        doi: if search.doi.is_empty() { None } else { Some(search.doi) },
        url: search.url,
        abstract_text: search.abstract_text,
        arxiv_id: Some(arxiv_id.to_string()),
    })
}

/// Crossref REST: `GET /works/<doi>` — title, authors, container-title, the
/// published year (print → online → issued), abstract (JATS, tags stripped).
async fn fetch_crossref(
    client: &reqwest::Client,
    bases: &ApiBases,
    doi: &str,
) -> Result<ResolvedPaper, ResolverError> {
    let url = format!("{}/works/{}", bases.crossref, urlenc(doi));
    let value = get_json(client, &url).await?;
    let message = value.get("message").cloned().ok_or_else(|| {
        ResolverError::Failed(format!("Crossref's answer carries no message for `{doi}`"))
    })?;
    let title = message
        .pointer("/title/0")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        return Err(ResolverError::Failed(format!(
            "Crossref has no title for DOI `{doi}`"
        )));
    }
    let authors = message
        .get("author")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let family = a.get("family").and_then(|v| v.as_str()).unwrap_or("");
                    let given = a.get("given").and_then(|v| v.as_str()).unwrap_or("");
                    let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let full = if !name.is_empty() {
                        name.to_string()
                    } else if !family.is_empty() {
                        format!("{family}{}", if given.is_empty() { String::new() } else { format!(", {given}") })
                    } else {
                        String::new()
                    };
                    if full.is_empty() { None } else { Some(full) }
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let year = [
        "/published/print/date-parts/0/0",
        "/published/online/date-parts/0/0",
        "/issued/date-parts/0/0",
    ]
    .iter()
    .find_map(|ptr| message.pointer(ptr).and_then(|v| v.as_i64()));
    let venue = message
        .pointer("/container-title/0")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let abstract_text = message
        .get("abstract")
        .and_then(|v| v.as_str())
        .map(strip_html_tags)
        .filter(|s| !s.is_empty());
    Ok(ResolvedPaper {
        source: "doi".into(),
        title,
        authors,
        year,
        venue,
        doi: Some(doi.to_string()),
        url: format!("https://doi.org/{doi}"),
        abstract_text,
        arxiv_id: None,
    })
}

/// PubMed E-utilities esummary (the documented endpoint, answered as JSON):
/// title, authors, journal, year, DOI.
async fn fetch_pubmed(
    client: &reqwest::Client,
    bases: &ApiBases,
    id: &str,
) -> Result<ResolvedPaper, ResolverError> {
    let url = format!("{}/esummary.fcgi?db=pubmed&id={}&retmode=json", bases.pubmed, id);
    let value = get_json(client, &url).await?;
    let result = value.get("result").cloned().ok_or_else(|| {
        ResolverError::Failed(format!("PubMed's esummary carries no result for `{id}`"))
    })?;
    if result.get("error").is_some() {
        return Err(ResolverError::Failed(format!("PubMed has no paper with id `{id}`")));
    }
    let item = result.get(id).cloned().ok_or_else(|| {
        ResolverError::Failed(format!("PubMed's esummary carries no entry `{id}`"))
    })?;
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        return Err(ResolverError::Failed(format!("PubMed's entry `{id}` has no title")));
    }
    let authors = item
        .get("authors")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let year = item
        .get("pubdate")
        .and_then(|v| v.as_str())
        .and_then(|s| s.get(0..4))
        .and_then(|s| s.parse().ok());
    let venue = item
        .get("fulljournalname")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let doi = item
        .get("elocationid")
        .and_then(|v| v.as_str())
        .and_then(|s| s.strip_prefix("doi: "))
        .map(|s| s.to_string())
        .or_else(|| {
            item.get("articleids")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|a| a.get("idtype").and_then(|v| v.as_str()) == Some("doi"))
                        .and_then(|a| a.get("value").and_then(|v| v.as_str()))
                        .map(|s| s.to_string())
                })
        })
        .filter(|d| looks_like_doi(d));
    Ok(ResolvedPaper {
        source: "pubmed".into(),
        title,
        authors,
        year,
        venue,
        doi,
        url: format!("https://pubmed.ncbi.nlm.nih.gov/{id}/"),
        abstract_text: None,
        arxiv_id: None,
    })
}

/// Semantic Scholar graph API: `GET /paper/<id>?fields=…`.
async fn fetch_s2(
    client: &reqwest::Client,
    bases: &ApiBases,
    id: &str,
) -> Result<ResolvedPaper, ResolverError> {
    let url = format!(
        "{}/paper/{}?fields=title,authors.name,year,venue,externalIds,abstract,url",
        bases.s2,
        urlenc(id)
    );
    let value = get_json(client, &url).await?;
    let title = value
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if title.is_empty() {
        return Err(ResolverError::Failed(format!(
            "Semantic Scholar has no paper with id `{id}`"
        )));
    }
    let authors = value
        .get("authors")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let venue = value
        .get("venue")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let doi = value
        .pointer("/externalIds/DOI")
        .and_then(|v| v.as_str())
        .filter(|d| looks_like_doi(d))
        .map(|s| s.to_string());
    let arxiv_id = value
        .pointer("/externalIds/ArXiv")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let abstract_text = value
        .get("abstract")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty());
    let url = value
        .get("url")
        .and_then(|v| v.as_str())
        .filter(|u| !u.trim().is_empty())
        .map(|u| u.to_string())
        .unwrap_or_else(|| format!("https://www.semanticscholar.org/paper/{id}"));
    Ok(ResolvedPaper {
        source: "s2".into(),
        title,
        authors,
        year: value.get("year").and_then(|v| v.as_i64()),
        venue,
        doi,
        url,
        abstract_text,
        arxiv_id,
    })
}

/// OpenAlex by DOI: `GET /works/doi:<doi>`.
async fn fetch_openalex_by_doi(
    client: &reqwest::Client,
    bases: &ApiBases,
    doi: &str,
) -> Result<ResolvedPaper, ResolverError> {
    let id = format!("doi:{doi}");
    let paper = fetch_openalex_work(client, bases, &id).await?;
    Ok(ResolvedPaper { source: "openalex".into(), ..paper })
}

/// The OpenAlex title search — the last resort, best effort: the first hit.
async fn fetch_openalex_search(
    client: &reqwest::Client,
    bases: &ApiBases,
    title: &str,
) -> Result<ResolvedPaper, ResolverError> {
    let url = format!("{}/works?search={}&per-page=1", bases.openalex, urlenc(title));
    let value = get_json(client, &url).await?;
    let hit = value
        .pointer("/results/0")
        .cloned()
        .ok_or_else(|| ResolverError::Failed(format!("no paper found for «{title}»")))?;
    parse_openalex_work(hit)
}

/// OpenAlex by work id (an OpenAlex id `W…`, or `doi:<doi>`).
async fn fetch_openalex_work(
    client: &reqwest::Client,
    bases: &ApiBases,
    id: &str,
) -> Result<ResolvedPaper, ResolverError> {
    let url = format!("{}/works/{}", bases.openalex, urlenc(id));
    let value = get_json(client, &url).await?;
    parse_openalex_work(value)
}

/// The OpenAlex work JSON → a resolved paper. The abstract comes as an
/// inverted index — reconstructed (positions → words) when present.
fn parse_openalex_work(value: serde_json::Value) -> Result<ResolvedPaper, ResolverError> {
    let title = value
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    if title.is_empty() {
        return Err(ResolverError::Failed(format!("OpenAlex's work `{id}` has no title")));
    }
    let authors = value
        .get("authorships")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.pointer("/author/display_name").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let venue = value
        .pointer("/primary_location/source/display_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let doi = value
        .get("doi")
        .and_then(|v| v.as_str())
        .and_then(|u| {
            u.strip_prefix("https://doi.org/")
                .or_else(|| u.strip_prefix("http://doi.org/"))
        })
        .filter(|d| looks_like_doi(d))
        .map(|s| s.to_string());
    let abstract_text = value
        .get("abstract_inverted_index")
        .and_then(|v| v.as_object())
        .map(|index| {
            let mut words: Vec<(usize, &String)> = index
                .iter()
                .flat_map(|(word, positions)| {
                    positions
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|p| p.as_u64().map(|pos| (pos as usize, word)))
                        .collect::<Vec<_>>()
                })
                .collect();
            words.sort_by_key(|(pos, _)| *pos);
            words.into_iter().map(|(_, word)| word.as_str()).collect::<Vec<_>>().join(" ")
        })
        .filter(|s| !s.is_empty());
    let url = match &doi {
        Some(doi) => format!("https://doi.org/{doi}"),
        None => id,
    };
    Ok(ResolvedPaper {
        source: "openalex".into(),
        title,
        authors,
        year: value.get("publication_year").and_then(|v| v.as_i64()),
        venue,
        doi,
        url,
        abstract_text,
        arxiv_id: None,
    })
}

/// Strip the JATS/HTML tags Crossref abstracts carry (`<p>…</p>`,
/// `<italic>…`), collapsing the whitespace the tags leave behind.
fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Percent-encode for a URL path or query value, keeping the delimiters the
/// identifiers carry (`/` in a DOI, `:` in a Semantic Scholar id) so the
/// APIs — and the test suite's fake-server routes — see them verbatim.
fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use serde_json::json;

    fn bases(addr: std::net::SocketAddr) -> ApiBases {
        let base = format!("http://{addr}");
        ApiBases {
            arxiv: format!("{base}/arxiv"),
            crossref: format!("{base}/crossref"),
            pubmed: format!("{base}/pubmed"),
            s2: format!("{base}/s2"),
            openalex: format!("{base}/openalex"),
        }
    }

    // ---- detection (pure, no network) ----

    #[test]
    fn arxiv_inputs_detect_to_the_arxiv_target() {
        for input in [
            "https://arxiv.org/abs/1706.03762",
            "http://arxiv.org/pdf/1706.03762v2.pdf",
            "https://export.arxiv.org/abs/cs/0601011",
            "  1706.03762  ",
            "cs/0601011",
        ] {
            assert_eq!(
                detect_target(input).unwrap(),
                Target::Arxiv(crate::domain::onboarding::parse_arxiv_url(input).unwrap()),
                "{input}"
            );
        }
    }

    #[test]
    fn bare_and_hosted_dois_detect_to_the_doi_target() {
        assert_eq!(
            detect_target("https://doi.org/10.1038/nature14539").unwrap(),
            Target::Doi("10.1038/nature14539".into())
        );
        assert_eq!(
            detect_target("http://dx.doi.org/10.48550/arXiv.1706.03762").unwrap(),
            Target::Doi("10.48550/arXiv.1706.03762".into())
        );
        // a DOI whose suffix itself carries a slash stays whole
        assert_eq!(
            detect_target("https://doi.org/10.1093/brain/awh270").unwrap(),
            Target::Doi("10.1093/brain/awh270".into())
        );
        assert_eq!(
            detect_target("10.1038/nature14539").unwrap(),
            Target::Doi("10.1038/nature14539".into())
        );
        // a trailing sentence period is not part of the DOI
        assert_eq!(
            detect_target("10.1038/nature14539.").unwrap(),
            Target::Doi("10.1038/nature14539".into())
        );
    }

    #[test]
    fn recognized_hosts_route_to_their_targets() {
        assert_eq!(
            detect_target("https://api.crossref.org/works/10.1038/nature14539").unwrap(),
            Target::Crossref("10.1038/nature14539".into())
        );
        assert_eq!(
            detect_target("https://pubmed.ncbi.nlm.nih.gov/31634576/").unwrap(),
            Target::Pubmed("31634576".into())
        );
        assert_eq!(
            detect_target("https://www.ncbi.nlm.nih.gov/pubmed/31634576").unwrap(),
            Target::Pubmed("31634576".into())
        );
        assert_eq!(
            detect_target("https://www.semanticscholar.org/paper/Attention-Is-All-You-Need-204e3073870fae3d05bcbc2f6a8e51b1ca4618ab").unwrap(),
            Target::S2("204e3073870fae3d05bcbc2f6a8e51b1ca4618ab".into())
        );
        assert_eq!(
            detect_target("https://api.semanticscholar.org/graph/v1/paper/arXiv:1706.03762")
                .unwrap(),
            Target::S2("arXiv:1706.03762".into())
        );
        assert_eq!(
            detect_target("https://api.openalex.org/works/W2741809807").unwrap(),
            Target::Openalex("W2741809807".into())
        );
    }

    #[test]
    fn unrecognized_urls_with_an_embedded_doi_fall_back_to_openalex() {
        // the common journal-link shapes carry the DOI in the path (or query)
        assert_eq!(
            detect_target("https://link.springer.com/article/10.1007/s00425-020-03430-6").unwrap(),
            Target::OpenalexDoi("10.1007/s00425-020-03430-6".into())
        );
        assert_eq!(
            detect_target("https://onlinelibrary.wiley.com/doi/10.1111/j.1467-9547.2007.01011.x")
                .unwrap(),
            Target::OpenalexDoi("10.1111/j.1467-9547.2007.01011.x".into())
        );
        assert_eq!(
            detect_target("https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0123456")
                .unwrap(),
            Target::OpenalexDoi("10.1371/journal.pone.0123456".into())
        );
        // trailing site sections are not part of the DOI
        assert_eq!(
            detect_target("https://doi.org/10.1093/brain/awh270").unwrap(),
            Target::Doi("10.1093/brain/awh270".into())
        );
        // a link with no DOI in it is honestly unsupported
        let err = detect_target("https://example.com/paper").unwrap_err();
        assert!(err.to_string().starts_with("unsupported_source:"), "{err}");
        assert!(
            err.to_string().contains("arXiv") && err.to_string().contains("doi.org"),
            "the refusal lists what IS supported: {err}"
        );
    }

    #[test]
    fn free_text_routes_to_the_title_search_and_single_words_are_refused() {
        assert_eq!(
            detect_target("attention is all you need").unwrap(),
            Target::OpenalexSearch("attention is all you need".into())
        );
        // a title with a period is free text, not a host
        assert_eq!(
            detect_target("attention is all you need. deep learning").unwrap(),
            Target::OpenalexSearch("attention is all you need. deep learning".into())
        );
        // single words, empty input and non-http schemes are honestly refused
        for bad in ["hello", "ftp://arxiv.org/abs/1706.03762", "  "] {
            let err = detect_target(bad).unwrap_err();
            assert!(
                err.to_string().starts_with("unsupported_source:")
                    || err.to_string().starts_with("invalid_url:"),
                "{bad:?} => {err}"
            );
        }
    }

    // ---- per-source resolution over a local fake server ----

    #[tokio::test]
    async fn an_arxiv_link_resolves_through_the_arxiv_api() {
        let atom = r#"<?xml version="1.0"?>
        <feed><entry>
          <id>http://arxiv.org/abs/1706.03762</id>
          <title>Paper Title From arXiv</title>
          <author><name>A. Author</name></author>
          <author><name>B. Author</name></author>
          <published>2017-06-12T00:00:00Z</published>
          <summary>The summary text.</summary>
        </entry></feed>"#;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app =
            axum::Router::new().route("/arxiv/query", get(move || async move { atom.to_string() }));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let paper = resolve_link_with(&bases(addr), "https://arxiv.org/abs/1706.03762")
            .await
            .unwrap();
        assert_eq!(paper.source, "arxiv");
        assert_eq!(paper.title, "Paper Title From arXiv");
        assert_eq!(paper.authors, "A. Author, B. Author");
        assert_eq!(paper.year, Some(2017));
        assert_eq!(paper.arxiv_id.as_deref(), Some("1706.03762"));
        assert_eq!(paper.url, "https://arxiv.org/abs/1706.03762");
        assert_eq!(paper.doi.as_deref(), Some("10.48550/arXiv.1706.03762"));
    }

    #[tokio::test]
    async fn a_doi_link_resolves_through_crossref() {
        let body = json!({
            "status": "ok",
            "message": {
                "title": ["Deep learning"],
                "author": [
                    {"given": "Yann", "family": "LeCun"},
                    {"given": "Yoshua", "family": "Bengio"},
                    {"given": "Geoffrey", "family": "Hinton"}
                ],
                "container-title": ["Nature"],
                "published": {"print": {"date-parts": [[2015, 5, 28]]}},
                "DOI": "10.1038/nature14539",
                "URL": "https://doi.org/10.1038/nature14539",
                "abstract": "<p>Deep learning allows computational models that are composed of <italic>multiple</italic> processing layers.</p>"
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/crossref/works/10.1038/nature14539",
            get(|| async move { axum::Json(body) }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let paper = resolve_link_with(&bases(addr), "https://doi.org/10.1038/nature14539")
            .await
            .unwrap();
        assert_eq!(paper.source, "doi");
        assert_eq!(paper.title, "Deep learning");
        assert_eq!(paper.authors, "LeCun, Yann, Bengio, Yoshua, Hinton, Geoffrey");
        assert_eq!(paper.venue, "Nature");
        assert_eq!(paper.year, Some(2015));
        assert_eq!(paper.doi.as_deref(), Some("10.1038/nature14539"));
        assert_eq!(paper.url, "https://doi.org/10.1038/nature14539");
        assert_eq!(
            paper.abstract_text.as_deref(),
            Some("Deep learning allows computational models that are composed of multiple processing layers.")
        );
        // a direct api.crossref.org link resolves through the same API,
        // badged crossref
        let paper = resolve_link_with(&bases(addr), "https://api.crossref.org/works/10.1038/nature14539")
            .await
            .unwrap();
        assert_eq!(paper.source, "crossref");
        assert_eq!(paper.title, "Deep learning");
    }

    #[tokio::test]
    async fn a_bare_doi_also_resolves_and_the_openalex_fallback_covers_a_crossref_miss() {
        // Crossref answers nothing for this DOI; OpenAlex has the work.
        let openalex = json!({
            "id": "https://openalex.org/W2741809807",
            "display_name": "Deep learning",
            "authorships": [
                {"author": {"display_name": "Yann LeCun"}},
                {"author": {"display_name": "Yoshua Bengio"}}
            ],
            "publication_year": 2015,
            "primary_location": {"source": {"display_name": "Nature"}},
            "doi": "https://doi.org/10.1038/missing",
            "abstract_inverted_index": {"Deep": [0], "learning": [1], "works": [2]}
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new()
            .route(
                "/crossref/works/10.1038/missing",
                get(|| async move {
                    (
                        axum::http::StatusCode::NOT_FOUND,
                        "resource not found",
                    )
                }),
            )
            .route(
                "/openalex/works/doi:10.1038/missing",
                get(|| async move { axum::Json(openalex) }),
            );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let paper = resolve_link_with(&bases(addr), "10.1038/missing").await.unwrap();
        // the paste targeted a DOI — the fallback keeps the honest target
        assert_eq!(paper.source, "doi");
        assert_eq!(paper.title, "Deep learning");
        assert_eq!(paper.authors, "Yann LeCun, Yoshua Bengio");
        assert_eq!(paper.year, Some(2015));
        assert_eq!(paper.venue, "Nature");
        assert_eq!(paper.abstract_text.as_deref(), Some("Deep learning works"));
    }

    #[tokio::test]
    async fn a_pubmed_link_resolves_through_eutils_esummary() {
        let body = json!({
            "result": {
                "31634576": {
                    "title": "A PubMed Paper Title",
                    "authors": [{"name": "Kim S"}, {"name": "Lee J"}],
                    "fulljournalname": "N Engl J Med",
                    "pubdate": "2019 Oct 24",
                    "elocationid": "doi: 10.1056/NEJMoa1901111",
                    "articleids": [{"idtype": "pubmed", "value": "31634576"}]
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/pubmed/esummary.fcgi",
            get(|| async move { axum::Json(body) }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let paper = resolve_link_with(&bases(addr), "https://pubmed.ncbi.nlm.nih.gov/31634576/")
            .await
            .unwrap();
        assert_eq!(paper.source, "pubmed");
        assert_eq!(paper.title, "A PubMed Paper Title");
        assert_eq!(paper.authors, "Kim S, Lee J");
        assert_eq!(paper.venue, "N Engl J Med");
        assert_eq!(paper.year, Some(2019));
        assert_eq!(paper.doi.as_deref(), Some("10.1056/NEJMoa1901111"));
        assert_eq!(paper.url, "https://pubmed.ncbi.nlm.nih.gov/31634576/");
        // PubMed has no such paper → the honest failure names the id
        let body = json!({"result": {"error": "cannot get document summary"}});
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/pubmed/esummary.fcgi",
            get(|| async move { axum::Json(body) }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let err = resolve_link_with(&bases(addr), "https://pubmed.ncbi.nlm.nih.gov/99999999/")
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("resolve_failed:"), "{err}");
        assert!(err.to_string().contains("99999999"), "{err}");
    }

    #[tokio::test]
    async fn a_semantic_scholar_link_resolves_through_the_graph_api() {
        let body = json!({
            "paperId": "204e3073870fae3d05bcbc2f6a8e51b1ca4618ab",
            "title": "Attention Is All You Need",
            "authors": [{"name": "Ashish Vaswani"}, {"name": "Noam Shazeer"}],
            "year": 2017,
            "venue": "NeurIPS",
            "externalIds": {"DOI": "10.48550/arXiv.1706.03762", "ArXiv": "1706.03762"},
            "abstract": "The dominant sequence transduction models.",
            "url": "https://www.semanticscholar.org/paper/204e3073870fae3d05bcbc2f6a8e51b1ca4618ab"
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/s2/paper/204e3073870fae3d05bcbc2f6a8e51b1ca4618ab",
            get(|| async move { axum::Json(body) }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let paper = resolve_link_with(
            &bases(addr),
            "https://www.semanticscholar.org/paper/Attention-Is-All-You-Need-204e3073870fae3d05bcbc2f6a8e51b1ca4618ab",
        )
        .await
        .unwrap();
        assert_eq!(paper.source, "s2");
        assert_eq!(paper.title, "Attention Is All You Need");
        assert_eq!(paper.authors, "Ashish Vaswani, Noam Shazeer");
        assert_eq!(paper.year, Some(2017));
        assert_eq!(paper.venue, "NeurIPS");
        assert_eq!(paper.doi.as_deref(), Some("10.48550/arXiv.1706.03762"));
        assert_eq!(paper.arxiv_id.as_deref(), Some("1706.03762"));
    }

    #[tokio::test]
    async fn an_openalex_link_and_a_journal_link_with_a_doi_resolve_through_openalex() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new()
            .route(
                "/openalex/works/W2741809807",
                get(|| async move {
                    axum::Json(json!({
                        "id": "https://openalex.org/W2741809807",
                        "display_name": "An OpenAlex Work",
                        "authorships": [{"author": {"display_name": "An Author"}}],
                        "publication_year": 2020,
                        "primary_location": {"source": {"display_name": "A Journal"}},
                        "doi": "https://doi.org/10.1007/s00425-020-03430-6"
                    }))
                }),
            )
            .route(
                "/openalex/works/doi:10.1007/s00425-020-03430-6",
                get(|| async move {
                    axum::Json(json!({
                        "id": "https://openalex.org/W999",
                        "display_name": "A Journal Paper",
                        "authorships": [],
                        "publication_year": 2020,
                        "primary_location": {"source": {"display_name": "Planta"}},
                        "doi": "https://doi.org/10.1007/s00425-020-03430-6"
                    }))
                }),
            );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        // a direct OpenAlex link
        let paper = resolve_link_with(&bases(addr), "https://api.openalex.org/works/W2741809807")
            .await
            .unwrap();
        assert_eq!(paper.source, "openalex");
        assert_eq!(paper.title, "An OpenAlex Work");
        assert_eq!(paper.year, Some(2020));
        // a journal link carrying its DOI in the path → OpenAlex by DOI
        let paper = resolve_link_with(
            &bases(addr),
            "https://link.springer.com/article/10.1007/s00425-020-03430-6",
        )
        .await
        .unwrap();
        assert_eq!(paper.source, "openalex");
        assert_eq!(paper.title, "A Journal Paper");
        assert_eq!(paper.venue, "Planta");
        assert_eq!(paper.doi.as_deref(), Some("10.1007/s00425-020-03430-6"));
    }

    #[tokio::test]
    async fn free_text_resolves_through_the_openalex_title_search() {
        let body = json!({
            "results": [{
                "id": "https://openalex.org/W2741809807",
                "display_name": "Deep learning",
                "authorships": [{"author": {"display_name": "Yann LeCun"}}],
                "publication_year": 2015,
                "primary_location": {"source": {"display_name": "Nature"}},
                "doi": "https://doi.org/10.1038/nature14539"
            }]
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let seen_clone = seen.clone();
        let app = axum::Router::new().route(
            "/openalex/works",
            get(
                move |q: axum::extract::Query<std::collections::HashMap<String, String>>| {
                    let seen = seen_clone.clone();
                    async move {
                        *seen.lock().unwrap() = q.0.get("search").cloned();
                        axum::Json(body)
                    }
                },
            ),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let paper = resolve_link_with(&bases(addr), "deep learning nature paper").await.unwrap();
        assert_eq!(paper.source, "openalex");
        assert_eq!(paper.title, "Deep learning");
        assert_eq!(paper.year, Some(2015));
        assert_eq!(
            seen.lock().unwrap().as_deref(),
            Some("deep learning nature paper"),
            "the title rode the search query"
        );
        // nothing found → the honest failure
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().route(
            "/openalex/works",
            get(|| async move { axum::Json(json!({"results": []})) }),
        );
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let err = resolve_link_with(&bases(addr), "no such paper anywhere")
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("resolve_failed:"), "{err}");
    }

    #[tokio::test]
    async fn unsupported_links_and_down_endpoints_are_honest_failures() {
        // an unsupported URL never fabricates metadata
        let err = resolve_link("https://example.com/paper").await.unwrap_err();
        assert!(err.to_string().starts_with("unsupported_source:"), "{err}");
        assert!(
            err.to_string().contains("arXiv") && err.to_string().contains("PubMed"),
            "the refusal lists the supported sources: {err}"
        );
        // a down endpoint is the typed resolve_failed, with the reason
        let gone = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gone_addr = gone.local_addr().unwrap();
        drop(gone);
        let err = resolve_link_with(&bases(gone_addr), "https://pubmed.ncbi.nlm.nih.gov/31634576/")
            .await
            .unwrap_err();
        assert!(err.to_string().starts_with("resolve_failed:"), "{err}");
        // a DOI nothing has resolves to neither service: both reasons named
        let err = resolve_link_with(&bases(gone_addr), "10.1038/nature14539")
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.starts_with("resolve_failed:"), "{msg}");
        assert!(msg.contains("Crossref") && msg.contains("OpenAlex"), "{msg}");
    }
}
