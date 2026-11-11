# Security policy

Cassiopeia reads data from external sources, transforms it into NGSI-LD entities and writes them to files or to a context broker, so a flaw in it can corrupt the context data your platform trusts or expose the credentials used to reach it. This document explains how to report security vulnerabilities in Cassiopeia responsibly.

---

## Supported versions

Only the latest release receives security fixes.

---

## Reporting a vulnerability

**Please do not report security vulnerabilities through public GitHub issues.**

Report privately through [GitHub private vulnerability reporting](https://github.com/vela-tools/cassiopeia/security/advisories/new), with:

- A description of the vulnerability
- Steps to reproduce, including the source data, the manifest and the command involved
- The version from `cassiopeia --version`
- The context broker and broker version Cassiopeia writes to, if the behaviour depends on it
- Any potential impact you have identified
- Optionally, a suggested fix

Leave out real credentials, tokens and personal data. A reproduction with a small synthetic source file is ideal.

You should receive an acknowledgement within **2 working days** (Monday to Friday, EU CET/CEST hours).

After triage, we will:

1. Confirm the vulnerability and determine its severity.
2. Develop and test a fix.
3. Coordinate a release that includes the fix.
4. Publicly disclose the vulnerability after users have had reasonable time to update.

---

## Disclosure timeline

We aim for the following timeline, though complex issues may take longer:

| Phase | Target |
|---|---|
| Initial response | 2 working days |
| Triage and severity assessment | 5 working days |
| Fix development and testing | 30 days for high/critical |
| Public disclosure | Within 90 days, or after fix is widely deployed |

We will keep you informed at each stage.

---

## Scope

This security policy covers:

- Code and dependencies in this repository
- Build and release artefacts published from this repository
- Documentation that could mislead users into insecure configurations

In Cassiopeia, that includes in particular:

- A crafted source file (CSV, JSON, GeoJSON, GBFS, GTFS or another supported format) that crashes, hangs or exhausts the memory of a run
- A crafted source archive or remote resource that makes Cassiopeia read or write outside the paths it was given
- A broker credential (an API key or a token) appearing in a log, an error message, a report file or an output file
- A manifest or template that executes code or reaches resources beyond what it declares
- Output that carries data from a source the manifest did not select

This policy does **not** cover:

- Vulnerabilities in the context broker itself, or in third-party software Cassiopeia depends on (please report those upstream)
- Incorrect or incomplete mapping of valid source data, which is a bug report
- Issues with deployment infrastructure operated by individual users
- Social engineering, physical attacks, or denial-of-service against velacontext.com

---

## Recognition

We are happy to credit reporters in the release notes of the fix, unless you prefer to remain anonymous. Let us know your preference when you report.

We do not currently operate a paid bug bounty programme, but genuine security improvements are deeply appreciated and may be eligible for swag, public recognition, or commercial discounts on Vela Context Data Hub.

---

## Contact

- **Security reports:** [GitHub private vulnerability reporting](https://github.com/vela-tools/cassiopeia/security/advisories/new)
- **General contact:** info@senlab.io
- **Commercial:** [velacontext.com/contact](https://velacontext.com/contactus)
