# Notice

Cassiopeia is licensed under the European Union Public Licence v. 1.2. See `LICENSE.md` for the full text.

## Trademarks

Vela, Cassiopeia, and the associated logos are trademarks of SenLab d.o.o. All rights in these marks are reserved.

The EUPL-1.2 grants rights in copyright, not in trademarks. Article 5, under Legal Protection, says so directly: "This Licence does not grant permission to use the trade names, trademarks, service marks, or names of the Licensor, except as required for reasonable and customary use in describing the origin of the Work and reproducing the content of the copyright notice."

You may use the names to say where your software came from: "based on Cassiopeia", "a fork of Cassiopeia", "compatible with Cassiopeia". That is descriptive use and needs no permission.

You may not use the names or the logos as the name or branding of your own distribution, product, or service, and you may not use them in a way that suggests SenLab d.o.o. endorses, maintains, or is the source of your version. A modified version distributed to the public must carry a different name.

Article 5, under Attribution right, is what makes this file travel with the code: the Licensee "shall keep intact all copyright, patent or trademarks notices" and "must include a copy of such notices and a copy of the Licence with every copy of the Work he/she distributes or communicates". So this notice must accompany the work and any derivative of it.

Any other use of the marks needs written permission, which you can request at info@senlab.io.

## Third-party material

The embedded NGSI-LD v1.9 core `@context`, as ETSI publishes it at `https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld`. It is copyright ETSI and distributed under the [BSD 3-Clause licence](crates/cassiopeia-value-type-expansion/contexts/LICENSE).

The QUDT unit table in `crates/cassiopeia-unit-notation/src/qudt/generated.rs` is adapted from QUDT, Quantities, Units, Dimensions and Types, by QUDT.org (<https://qudt.org>), release 3.5.3, licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). It keeps only the units that carry a `qudt:uneceCommonCode`, with their codes, symbols, and English labels, and composes those texts to Unicode Normalization Form C. Cassiopeia reads some of the codes differently from QUDT, as `crates/cassiopeia-unit-notation/src/qudt/correction.rs` lists.

The UCUM codes in that table, the `qudt:ucumCode` values, are copyright Regenstrief Institute, Inc., and used under the [UCUM License](https://ucum.org/license), which disclaims all warranties.
