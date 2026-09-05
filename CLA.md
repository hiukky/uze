<!-- PENDING LEGAL REVIEW — DO NOT PUBLISH -->

# Individual Contributor License Agreement

**This document is not in force and must not be presented to contributors
until a lawyer has reviewed it.** It is a scaffold: the Apache Software
Foundation's ICLA v2.2 adapted to a project that is not the ASF, plus two
clauses the maintainer asked for. Every departure from the ASF text is
listed below so a reviewer can diff it against the original rather than
read it cold.

Base document: the Apache Individual Contributor License Agreement V2.2,
<https://www.apache.org/licenses/icla.pdf>.

Departures from that text, all of them deliberate:

1. **"The Apache Software Foundation" / "the Foundation" are replaced
   throughout by Romullo Sousa (hiukky)**, the copyright holder named in
   `LICENSE` and `NOTICE`. Without this the agreement grants rights to the
   ASF, which has nothing to do with this project.
2. **The nonprofit undertaking is removed.** The ASF text promises in
   return not to use Contributions "in a way that is contrary to the public
   benefit or inconsistent with its nonprofit status and bylaws". uze has
   no nonprofit status and no bylaws, so the sentence could not be honoured
   as written.
3. **Clause 2 gains a sentence** placing future versions under any licence,
   including commercial ones. The ASF grant already includes the right to
   sublicense; this states the consequence outright rather than leaving a
   contributor to infer it.
4. **Clause 9 is new**: the contributor keeps the copyright, may reuse the
   contribution freely, and nothing here transfers ownership. This is a
   licence grant, not an assignment, and the document should say so in its
   own clause rather than only in the preamble.
5. **The ASF's signing mechanics are removed** — the printed form fields
   and the instruction to email a PDF to `secretary@apache.org`. Signing
   here happens by comment on a pull request; see the end of this document.
6. **No governing law and no venue clause.** Deliberately not drafted:
   that is counsel's to choose, not this scaffold's to guess.
7. **Clause 10 is new**: the Agreement and the licences under it pass to a
   successor in the ownership of the Work. The ASF never needed this — a
   foundation does not change legal form. A holder who is a person today and
   an entity tomorrow does, and without this the contributor's grant would
   have to be chased down or re-signed at exactly the moment that is hardest.
8. **A sentence is given back where the nonprofit undertaking was removed.**
   Departure 2 took out the only thing the counterparty promised in return,
   which is the shape that makes a broad-grant CLA read as one-sided. What
   replaces it promises nothing new: a released version keeps the licence it
   was released under, which is already true of an irrevocable Apache-2.0
   grant, and clause 2 governs future versions rather than that one.
9. **Clauses 9 and 10 carry an `ADDED — NOT ASF TEXT` marker** in the body,
   so a reader inside the document can tell what is not the ICLA without
   scrolling back to this list.

Not a departure, but worth knowing before someone asks: clause 4 offers a
contributor whose employer holds rights three routes, and the third is a
Corporate CLA. **No Corporate CLA exists for this project today**, so that
route is unavailable rather than undocumented — the other two work as
written. The sentence stays as the ICLA wrote it: deleting it would widen
the diff against the original to fix nothing, and it is the sentence a
corporate contributor will point at when one is finally needed.

---

You accept and agree to the following terms and conditions for Your
Contributions (present and future) that you submit to
Romullo Sousa (hiukky). In return, a version of the Work that has been
released with Your Contribution in it stays licensed under the licence it
was released under; a later change of licence applies to later versions, not
to that one. Except for the license granted herein to
Romullo Sousa (hiukky) and recipients of software distributed by
Romullo Sousa (hiukky), You reserve all right, title, and interest in and
to Your Contributions.

**1. Definitions.**

"You" (or "Your") shall mean the copyright owner or legal entity authorized
by the copyright owner that is making this Agreement with
Romullo Sousa (hiukky). For legal entities, the entity making a
Contribution and all other entities that control, are controlled by, or are
under common control with that entity are considered to be a single
Contributor. For the purposes of this definition, "control" means (i) the
power, direct or indirect, to cause the direction or management of such
entity, whether by contract or otherwise, or (ii) ownership of fifty
percent (50%) or more of the outstanding shares, or (iii) beneficial
ownership of such entity.

"Contribution" shall mean any original work of authorship, including any
modifications or additions to an existing work, that is intentionally
submitted by You to Romullo Sousa (hiukky) for inclusion in, or
documentation of, any of the products owned or managed by
Romullo Sousa (hiukky) (the "Work"). For the purposes of this definition,
"submitted" means any form of electronic, verbal, or written communication
sent to Romullo Sousa (hiukky) or its representatives, including but not
limited to communication on electronic mailing lists, source code control
systems, and issue tracking systems that are managed by, or on behalf of,
Romullo Sousa (hiukky) for the purpose of discussing and improving the
Work, but excluding communication that is conspicuously marked or otherwise
designated in writing by You as "Not a Contribution."

**2. Grant of Copyright License.**

Subject to the terms and conditions of this Agreement, You hereby grant to
Romullo Sousa (hiukky) and to recipients of software distributed by
Romullo Sousa (hiukky) a perpetual, worldwide, non-exclusive, no-charge,
royalty-free, irrevocable copyright license to reproduce, prepare
derivative works of, publicly display, publicly perform, sublicense, and
distribute Your Contributions and such derivative works.

You further agree that Your Contributions may be included in future
versions of the Work distributed under any licence chosen by
Romullo Sousa (hiukky), including a commercial licence, and that no further
permission from You is required for that.

**3. Grant of Patent License.**

Subject to the terms and conditions of this Agreement, You hereby grant to
Romullo Sousa (hiukky) and to recipients of software distributed by
Romullo Sousa (hiukky) a perpetual, worldwide, non-exclusive, no-charge,
royalty-free, irrevocable (except as stated in this section) patent license
to make, have made, use, offer to sell, sell, import, and otherwise
transfer the Work, where such license applies only to those patent claims
licensable by You that are necessarily infringed by Your Contribution(s)
alone or by combination of Your Contribution(s) with the Work to which such
Contribution(s) was submitted. If any entity institutes patent litigation
against You or any other entity (including a cross-claim or counterclaim in
a lawsuit) alleging that your Contribution, or the Work to which you have
contributed, constitutes direct or contributory patent infringement, then
any patent licenses granted to that entity under this Agreement for that
Contribution or Work shall terminate as of the date such litigation is
filed.

**4.** You represent that you are legally entitled to grant the above
license. If your employer(s) has rights to intellectual property that you
create that includes your Contributions, you represent that you have
received permission to make Contributions on behalf of that employer, that
your employer has waived such rights for your Contributions to
Romullo Sousa (hiukky), or that your employer has executed a separate
Corporate CLA with Romullo Sousa (hiukky).

**5.** You represent that each of Your Contributions is Your original
creation (see section 7 for submissions on behalf of others). You represent
that Your Contribution submissions include complete details of any
third-party license or other restriction (including, but not limited to,
related patents and trademarks) of which you are personally aware and which
are associated with any part of Your Contributions.

**6.** You are not expected to provide support for Your Contributions,
except to the extent You desire to provide support. You may provide support
for free, for a fee, or not at all. Unless required by applicable law or
agreed to in writing, You provide Your Contributions on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied,
including, without limitation, any warranties or conditions of TITLE,
NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A PARTICULAR PURPOSE.

**7.** Should You wish to submit work that is not Your original creation,
You may submit it to Romullo Sousa (hiukky) separately from any
Contribution, identifying the complete details of its source and of any
license or other restriction (including, but not limited to, related
patents, trademarks, and license agreements) of which you are personally
aware, and conspicuously marking the work as "Submitted on behalf of a
third-party: [named here]".

**8.** You agree to notify Romullo Sousa (hiukky) of any facts or
circumstances of which you become aware that would make these
representations inaccurate in any respect.

<!-- ADDED — NOT ASF TEXT -->

**9. You keep your copyright.**

This Agreement grants a licence and transfers nothing. You remain the
copyright owner of Your Contributions. You may use, licence and distribute
Your Contributions for any other purpose, on any terms, without restriction
by or obligation to Romullo Sousa (hiukky). Nothing in this Agreement
assigns ownership of Your Contributions or of any intellectual property
right in them.

<!-- ADDED — NOT ASF TEXT -->

**10. Successors.**

Romullo Sousa (hiukky) may assign or transfer this Agreement, and the
licences granted under it, to a successor in the ownership of the Work,
including on a change of legal form, reorganisation, merger or acquisition.
No further permission from You is required, and the Agreement binds and
benefits the successor on the same terms.

---

## Signing

Signing is by comment on your pull request. A bot asks the first time you
open one, and the comment it asks for is recorded against your GitHub
account so it is asked only once:

```
I have read the CLA Document and I hereby sign the CLA
```

Signatures are stored outside this repository, in a repository the
maintainer controls.
