# Wiki Schema

## Domain

Nghien cuu san pham va ky thuat cho LuxePix, tap trung vao trai nghiem duyet
thu muc, quan ly tep, tim kiem san pham bang ma va tim anh tuong tu.

## Conventions

- File names: lowercase, hyphenated, no spaces.
- Wiki pages use YAML frontmatter.
- Use `[[wikilinks]]`; each entity, concept, comparison, or query page needs at
  least two outbound links.
- Raw source notes under `raw/` are immutable after ingestion.
- New pages must be added to `index.md`.
- Every ingest, query, update, or lint action must be appended to `log.md`.
- Claims synthesized from multiple sources include provenance markers.
- Dates use ISO `YYYY-MM-DD`.

## Frontmatter

```yaml
---
title: Page title
created: YYYY-MM-DD
updated: YYYY-MM-DD
type: entity | concept | comparison | query | summary
tags: [tag-a, tag-b]
sources: [raw/articles/source-note.md]
confidence: high | medium | low
---
```

## Tag Taxonomy

- Products: `luxepix`, `filebrowser`, `filebrowser-quantum`
- Platform: `desktop-app`, `web-app`, `windows`, `portable`
- Capability: `file-management`, `navigation`, `search`, `indexing`, `media`,
  `image-similarity`, `shortcuts`
- Quality: `ux`, `performance`, `safety`, `security`
- Meta: `comparison`, `roadmap`, `questionnaire`, `debugging`

Every page tag must appear above.

## Page Thresholds

- Create a page when a product or concept is central to the research.
- Add to an existing page for incremental evidence about the same subject.
- Split pages over about 200 lines.
- Do not create pages for passing implementation details.

## Update Policy

1. Prefer newer official documentation and release notes.
2. Keep version-specific facts with an exact date.
3. Record contradictions instead of silently replacing them.
4. Keep recommendations separate from sourced facts.

