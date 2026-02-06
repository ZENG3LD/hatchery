# Blizzard IP Legal Analysis: Using StarCraft Zerg Unit Names in Developer Tools

**Project Context**: Hatchery - A Rust CLI tool for swarm orchestration using mode names: Queen, Swarm Host, Brood Lord, Hatchery
**Research Date**: 2026-02-06
**Risk Assessment**: MODERATE to LOW (context-dependent)

## Executive Summary

**Bottom Line**: Using StarCraft Zerg unit names in a non-gaming developer tool is **legally gray but likely low-risk** for several reasons:

1. Individual unit names like "Hatchery," "Queen," "Swarm" are likely **NOT separately trademarked** (only "StarCraft" brand is)
2. Blizzard historically targets **game-related projects** (clones, servers, mods) - **no evidence of action against developer tools**
3. Terms like "Queen," "Swarm," "Hatchery" are **generic/descriptive** and weak for trademark protection
4. Your use case is **non-commercial, different industry** (dev tools vs. gaming)

**However**: Risk is NOT zero. Blizzard is protective of its IP and has sent C&Ds for name confusion even when projects weren't competitive.

---

## 1. What Blizzard Protects (Trademark & Copyright)

### 1.1 Registered Trademarks

**Confirmed Blizzard Trademarks:**
- **StarCraft®** (registered trademark, USPTO #2424142)
- **Blizzard®, Warcraft®, Diablo®, Hearthstone™**
- Logos, visual designs, game packaging

**What the StarCraft Trademark Covers:**
- Computer programs consisting of strategy games of interstellar combat
- Paper goods and toys
- The "StarCraft" brand name itself

### 1.2 Individual Unit Names (Hatchery, Brood Lord, etc.)

**Critical Finding**: No evidence found that individual unit names are separately trademarked.

- USPTO search returned **no results** for "Hatchery," "Brood Lord," "Swarm Host" registered to Blizzard
- Blizzard's trademark guidelines mention only **franchise names** (StarCraft, Warcraft, etc.) as protected marks
- Unit/character names fall under **copyright** (creative expression) rather than trademark

**Why This Matters**: Copyright protects the **creative expression** (visual design, lore, storyline) but is weaker for protecting **names alone** compared to trademarks.

### 1.3 Generic/Descriptive Terms Analysis

| Term | Genericness | Trademark Risk |
|------|-------------|----------------|
| **Hatchery** | High - common term for insect/animal breeding structures | Low |
| **Queen** | Very High - common word, used in countless contexts | Very Low |
| **Swarm** / **Swarm Host** | High - generic term for insect groups | Low |
| **Brood Lord** | Moderate - "Brood" and "Lord" are common, combination is more distinctive | Low-Moderate |
| **Zergling** | Low - likely coined by Blizzard, distinctive | Moderate-High |

**Legal Principle**: Generic or descriptive terms are **weak trademarks** and difficult to enforce. "Hatchery" and "Queen" are dictionary words used in many contexts unrelated to gaming.

---

## 2. When Blizzard WILL Take Action

### 2.1 Historical Enforcement Patterns

Blizzard actively enforces IP rights in these categories:

#### A. Game Clones / Competing Games
**Example**: **FreeCraft (2003)**
- Open-source Warcraft engine clone
- Received C&D for **name confusion** ("FreeCraft" too similar to "StarCraft"/"WarCraft")
- **Also** flagged for engine features too similar to Warcraft 2
- Developers renamed to "Stratagus" and continued legally

**Takeaway**: Blizzard protects against projects that could be confused as Blizzard products OR compete in the RTS gaming space.

#### B. Mods Using Franchise Names
**Example**: **World of StarCraft Mod (2011)**
- StarCraft II mod styled like World of Warcraft
- Received C&D specifically over **name** ("World of StarCraft")
- Blizzard stated goal was to "protect property names," not kill the mod itself

**Takeaway**: Blizzard cares deeply about **franchise name combinations** (e.g., "World of X," "StarCraft X").

#### C. Private Servers / Game Clones
**Examples**:
- **Turtle WoW (2025)**: Private WoW server, sued for "egregious copyright infringement"
- **Project Epoch (2025)**: Rogue WoW server, C&D sent
- **bnetd**: Battle.net clone, received legal action

**Takeaway**: Blizzard aggressively defends against unauthorized game servers/emulators.

#### D. Cheat Tools / Bots
**Examples**:
- **Bossland**: Cheat software, sued for copyright infringement and unfair competition
- **Overwatch cheat makers**: Sued in 2016

**Takeaway**: Tools that directly modify or interact with Blizzard games are targeted.

### 2.2 What Triggers Enforcement

Based on historical cases, Blizzard takes action when:

1. **Likelihood of confusion**: Product could be mistaken as a Blizzard product
2. **Same industry**: Gaming, entertainment, related digital goods
3. **Franchise name use**: Direct use of "StarCraft," "Warcraft," "Diablo," etc. in product name
4. **Commercial use**: Selling products or services using Blizzard IP
5. **Direct competition**: Product competes with or replaces Blizzard games/services

---

## 3. When Blizzard WON'T Care (Likely Safe Uses)

### 3.1 Non-Commercial Use
From Blizzard's trademark guidelines:
> "These marks may be used only for **non-commercial purposes**, except as permitted by the applicable Activity Policy."

**Your case**: Open-source project with no direct monetization = likely qualifies as non-commercial.

### 3.2 Different Industry / No Confusion
**Key Legal Principle**: Trademark infringement requires **"likelihood of confusion"** about the source of goods.

**Your case**:
- Industry: Developer tools / CLI automation (NOT gaming)
- No visual similarity to StarCraft
- No claims of affiliation with Blizzard
- Different target audience (developers, not gamers)

**Analogy**: A "Hatchery" restaurant or "Queen" brand mattresses wouldn't infringe StarCraft trademarks because they're different industries.

### 3.3 Educational / Reference Use
Blizzard's video policy explicitly supports:
> "Blizzard Entertainment supports the use of its game assets for **educational purposes**, and creators are welcome to create productions for school projects or master's theses."

**Your case**: Using names as conceptual reference (swarm orchestration inspired by Zerg units) could be argued as educational/reference.

### 3.4 Internal / Private Tools
No evidence of Blizzard pursuing **internal corporate tools** or **private projects** using game terminology.

### 3.5 Transformative / Parody Use
**Fair Use Doctrine**: Transformative uses (parody, criticism, commentary) are protected.

**Your case**: Arguable that using Zerg concepts for a swarm orchestration tool is **transformative** - taking biological/strategic concepts and applying them to software architecture.

---

## 4. Real-World Examples

### 4.1 Projects That Got C&D'd
| Project | Type | Reason | Outcome |
|---------|------|--------|---------|
| FreeCraft | Game engine clone | Name confusion + game similarity | Renamed to Stratagus |
| World of StarCraft | SC2 mod | Used "StarCraft" in name | C&D (mod could continue without name) |
| Turtle WoW | Private server | Replicated WoW game | Lawsuit filed |
| bnetd | Battle.net clone | Game server emulation | Legal action |

### 4.2 Projects Using StarCraft IP Safely
| Project | Type | Why It's Safe | Status |
|---------|------|---------------|--------|
| **Stratagus** | RTS engine | Renamed from FreeCraft, NO StarCraft branding | Active since 2004 |
| **BWAPI** | StarCraft modding API | Tool for users who own StarCraft, educational/research | Active, widely used |
| **SC-3DS** | SC port to 3DS | Open source, requires legal SC copy | Active on GitHub |
| **Open Source AI bots** | SC AI competitors | Educational/research, requires game ownership | Widely used in research |
| **StarCraft wikis/databases** | Fan sites | Reference/educational, no game cloning | Active for years |

**Key Pattern**: Projects are safer when:
1. They don't use "StarCraft" or "Warcraft" in the product name
2. They require users to own the original game
3. They're educational/research tools
4. They're not in the gaming industry

### 4.3 Non-Gaming Software Using Game Names
**Research Gap**: No cases found of Blizzard pursuing **non-gaming software** for using unit/character names.

**Comparable Cases**:
- **Ion Maiden (3D Realms) vs. Iron Maiden (band)**: Game name too similar to band trademark, sued. (Different from your case - both in entertainment industry)
- Generic terms like "Queen," "Hatchery" used in countless products across industries without issue

---

## 5. Specific Risk Assessment for "Hatchery"

### 5.1 Is "Hatchery" Specifically Risky?

**Factors Reducing Risk**:
1. **Generic term**: "Hatchery" is a common English word for animal breeding structures
2. **Not in game title**: Used in StarCraft but not a franchise name
3. **No trademark found**: USPTO search found no "Hatchery" trademark by Blizzard
4. **Different context**: Your use is for software orchestration, not gaming
5. **Weak trademark**: Generic/descriptive terms are hard to enforce

**Factors Increasing Risk**:
1. **Recognizable**: StarCraft players will recognize the reference
2. **Product name**: It's your main product name, not just an internal term
3. **Open acknowledgment**: If you explicitly say "inspired by StarCraft," you acknowledge the connection

### 5.2 Risk Matrix

| Factor | Weight | Assessment | Points |
|--------|--------|------------|--------|
| Same industry as SC2? | High | No (dev tools vs. gaming) | -3 |
| Commercial use? | High | No (open-source) | -3 |
| Uses "StarCraft" in name? | Very High | No | -5 |
| Generic/descriptive term? | Medium | Yes (Hatchery is generic) | -2 |
| Explicit acknowledgment of SC2? | Medium | Unknown (your docs) | 0 to +2 |
| Could cause confusion? | High | Low (different context) | -2 |
| Historical precedent? | Medium | None found for non-gaming tools | -2 |

**Score**: -17 to -15 (out of -21 to +21 scale)
**Risk Level**: **LOW to MODERATE**

### 5.3 Likelihood of Legal Action

**Very Unlikely** (<5% chance):
- Blizzard has never (publicly) pursued non-gaming software for unit name usage
- "Hatchery" is too generic to enforce strongly
- No commercial competition with Blizzard products

**Possible but Rare** (5-15% chance):
- Blizzard sends a friendly C&D requesting name change
- More likely if project becomes very visible/popular
- More likely if you explicitly market as "StarCraft-inspired"

**Extremely Unlikely** (lawsuit):
- No economic harm to Blizzard
- No industry overlap
- Generic term defense is strong

---

## 6. Safe Alternatives (If Needed)

### 6.1 Name Modification Strategies

If you want to further reduce risk:

| Strategy | Example | Risk Reduction |
|----------|---------|----------------|
| **One-word compounds** | "broodlord" (vs. "Brood Lord") | Moderate - more generic |
| **Spelling variations** | "Hatcherie," "Hatcheri" | Moderate - less direct connection |
| **Generic insect terms** | "Nest," "Colony," "Hive" | High - breaks SC2 connection |
| **Scientific names** | "Ovipositor," "Swarm Matrix" | High - no SC2 reference |
| **Different mythology** | Norse/Greek insect deities | High - completely different |

### 6.2 Mode Name Alternatives (Keeping Swarm Theme)

If you want to keep the biological orchestration theme without SC2 references:

| Current (SC2) | Alternative | Concept |
|---------------|-------------|---------|
| **Queen** | Matriarch, Overmind, Nexus | Central controller |
| **Swarm Host** | Colony, Hive, Cluster | Distributed workers |
| **Brood Lord** | Overseer, Coordinator, Orchestrator | High-level manager |
| **Hatchery** | Spawner, Incubator, Genesis | Creation/initialization |

**Recommendation**: If you go with alternatives, choose terms that:
1. Maintain the biological/insect metaphor
2. Are clearly generic (not coined by Blizzard)
3. Still evoke the right architectural concepts

---

## 7. Recommended Strategy

### 7.1 Proceed with "Hatchery" BUT Take Precautions

**Recommended Approach**:

1. **Use the names**: Risk is low enough to proceed with Hatchery, Queen, Swarm Host, Brood Lord
2. **Don't explicitly market as "StarCraft-inspired"**: Avoid phrases like "inspired by SC2 Zerg" in official docs
3. **Generic framing**: Frame names as **generic biological/swarm concepts**, not game references
4. **No visual references**: Don't use StarCraft imagery, logos, or visual designs
5. **Clear differentiation**: Make it obvious this is a developer tool, not a game
6. **Disclaimer**: Consider adding "All trademarks are property of their respective owners" in docs

### 7.2 If You Receive a C&D

**Response Plan**:
1. **Don't panic**: C&D is not a lawsuit, just a request
2. **Consult IP attorney**: Get professional advice (initial consult often free)
3. **Negotiate**: Blizzard may be satisfied with name change or disclaimer
4. **Rename if necessary**: Have backup names ready (see Section 6.2)
5. **Document**: Keep all communications for legal protection

### 7.3 Risk Mitigation Checklist

- [ ] Avoid using "StarCraft" or "Blizzard" in product name or primary branding
- [ ] Don't use StarCraft visual assets (images, logos, game art)
- [ ] Frame terminology as generic swarm/biological concepts
- [ ] Include trademark disclaimer in documentation
- [ ] Don't market as "StarCraft-based" or "StarCraft-inspired"
- [ ] Ensure project is clearly non-gaming and open-source
- [ ] Have backup names ready if needed
- [ ] Monitor for any IP complaints and respond promptly

---

## 8. Legal Considerations & Disclaimers

### 8.1 Trademark vs. Copyright

**Important Distinction**:
- **Trademark** protects brand names/logos used in commerce
  - Requires "likelihood of confusion" in same industry
  - Stronger for distinctive/invented terms, weaker for generic terms
- **Copyright** protects creative expression (art, code, storylines)
  - Names alone generally NOT copyrightable
  - Protects Zerg visual design, lore, game mechanics, but not the concept of "hatchery"

**Your case**: Trademark is the main concern, and risk is reduced by:
1. Generic terms
2. Different industry
3. No commercial conflict

### 8.2 Fair Use Defense

**Potential Fair Use Arguments**:
1. **Transformative use**: Applying gaming concepts to software architecture
2. **No market harm**: Developer tools don't compete with/replace StarCraft
3. **Different purpose**: Educational/functional vs. entertainment
4. **Minimal taking**: Only names, not artwork, code, or game mechanics

**Limitation**: Fair use is determined case-by-case in court - not a guaranteed defense.

### 8.3 Generic Term Defense

**Strong Defense**:
- "Hatchery," "Queen," "Swarm" are **dictionary words**
- Blizzard doesn't own exclusive rights to generic terms
- Many products use these terms across industries

**Example**: You can name a software tool "Queen" just like there's a band named Queen, a mattress brand, a chess piece, etc.

---

## 9. Conclusion & Recommendations

### 9.1 Final Risk Assessment

**Overall Risk**: **LOW to MODERATE**

**Breakdown**:
- **Legal risk**: Low (strong defenses, no precedent of enforcement in this context)
- **C&D risk**: Low-Moderate (possible if project becomes very visible, but unlikely)
- **Lawsuit risk**: Very Low (no economic or competitive harm to Blizzard)

### 9.2 Action Plan

**RECOMMENDED**:
✅ **Proceed with current names** (Hatchery, Queen, Swarm Host, Brood Lord)
✅ **Frame as generic swarm/biological architecture concepts**
✅ **Add trademark disclaimer** to documentation
✅ **Avoid explicit StarCraft marketing**
✅ **Prepare backup names** as contingency

**NOT RECOMMENDED**:
❌ Don't use "StarCraft" in product name
❌ Don't use Blizzard's visual assets
❌ Don't claim affiliation with or endorsement by Blizzard
❌ Don't market explicitly as "StarCraft-inspired"

### 9.3 When to Reconsider

Re-evaluate if:
1. Project becomes commercial (paid product/service)
2. You receive any IP complaint (C&D, trademark objection)
3. Project enters gaming/entertainment space
4. You want to use "StarCraft" in marketing
5. Blizzard updates its IP policy with new restrictions

### 9.4 Professional Legal Advice

**Disclaimer**: This research is for informational purposes only and is not legal advice. For definitive guidance:
- Consult a trademark/IP attorney
- Consider a professional trademark clearance search
- Get legal review if project becomes commercial

---

## Sources

1. [Blizzard Entertainment Trademark Usage Guidelines](https://www.blizzard.com/en-us/legal/38fd0408-8431-469a-99bc-2cd9eb9462c8/blizzard-entertainment-trademark-usage-guidelines)
2. [StarCraft Wiki: Copyrights](https://starcraft.fandom.com/wiki/StarCraft_Wiki:Copyrights)
3. [Blizzard Entertainment Logo And Trademark Guidelines](https://www.blizzard.com/en-gb/legal/8bcb0794-6641-4ce3-a573-8eb243bab342/blizzard-entertainment-logo-and-trademark-guidelines)
4. [Copyright Notices - Blizzard Entertainment](https://www.blizzard.com/en-us/legal/5515ca11-1c96-42a0-b853-e7876a0d19bf/copyright-notices)
5. [STARCRAFT - Blizzard Entertainment, Inc. Trademark Registration](https://uspto.report/TM/75979934)
6. [FreeCraft Cease and Desisted by Blizzard - Slashdot](https://games.slashdot.org/story/03/06/21/1323249/FreeCraft-Cease-and-Desisted-by-Blizzard)
7. [World of StarCraft Mod Gets C&D From Blizzard - Slashdot](https://games.slashdot.org/story/11/01/19/191257/world-of-starcraft-mod-gets-cd-from-blizzard)
8. [Another World of Warcraft rogue server, Project Epoch, has been smacked with a Blizzard cease and desist](https://massivelyop.com/2025/09/09/another-world-of-warcraft-rogue-server-project-epoch-has-been-smacked-with-a-blizzard-cease-and-desist/)
9. [The name of the game: Video game titles and trademark protection](https://newtech.law/en/articles/the-name-of-the-game-video-game-titles-and-trademark-protection)
10. [Video Game Trademark Ultimate Guide](https://strebecklaw.com/protect-video-game-trademark/)
11. [When video games meet IP law - WIPO](https://www.wipo.int/en/web/wipo-magazine/articles/when-video-games-meet-ip-law-41991)
12. [Can i use the name of a character/corporation from a video game as the brand of my own company? - Legal Answers](https://www.avvo.com/legal-answers/can-i-use-the-name-of-a-character-corporation-from-1996424.html)
13. [Blizzard Video Policy](https://www.blizzard.com/en-us/legal/dd76b654-f2c4-4aaa-ba49-ca3122de2376/blizzard-video-policy)
14. [Using Activision Blizzard's Intellectual Property - Blizzard Support](https://us.battle.net/support/en/article/267198)
15. [Stratagus - Wikipedia](https://en.wikipedia.org/wiki/Stratagus)
16. [GitHub - StarCraft Topics](https://github.com/topics/starcraft?l=c++)
17. [Open Source Games Like StarCraft](https://alternativeto.net/software/starcraft/?license=opensource)
18. [BWAPI: The Brood War API](https://bwapi.github.io/)
19. [Video Games and the law: Copyright, Trademark and Intellectual Property](https://newmediarights.org/guide/legal/Video_Games_law_Copyright_Trademark_Intellectual_Property)
20. [Intellectual property protection of video games - Wikipedia](https://en.wikipedia.org/wiki/Intellectual_property_protection_of_video_games)
21. [Copyright in Characters: What Can I Use? Part II](https://www.aspectlg.com/posts/copyright-in-characters-what-can-i-use-part-ii)
22. [Fair Use in Gaming Content – FAQS For Creators](https://cdas.com/fair-use-in-gaming-content-faqs-for-creators/)
23. [Getting Creative with Video Games: Copyright, Public Domain, and Fair Use](https://www.carltonfields.com/insights/publications/2019/video-games-copyright-public-domain-fair-use)

---

**Research Completed**: 2026-02-06
**Analyst**: Claude (Sonnet 4.5)
**Review Status**: Awaiting legal counsel review if commercial use planned