# Wirtschaft, Berufe und Bedürfnisse – Designentscheidungen

Stand: 2026-11-20. Ergebnis einer Diskussion über die Regeln in
`content/data/world.toml`. Noch nichts davon ist implementiert.

## Grundsätze

- **Emergent statt von Hand skaliert.** Gewerbe entstehen, wenn es Nachfrage
  gibt, und verschwinden wieder. Mechanismen reparieren, nicht die Welt für
  mehr Agenten von Hand umbauen. Harte Obergrenzen (`max_firms`, `max_houses`,
  `max_agents`) sind Sicherheitsnetze und sollten nie die greifende Grenze sein.
- **Durchsatz vor Realismus.**
  1. Kein Mechanismus pro Minute, nur pro Block oder Ereignis.
  2. Jede Entscheidung bewertet höchstens ~10 Optionen.
  3. Ganzzahl-Arithmetik und Tabellen.
  4. Was der Spieler nicht merkt, wird nicht modelliert.
  5. Pro Tick nur Bedürfnisse, pro Ereignis die Karriere.
- **Gleicher Seed.** Jede Zeitschleife startet exakt identisch. Ohne Eingriff
  verläuft jedes Leben gleich; Abweichungen kommen nur vom Spieler.
- **NPCs und Spieler laufen dieselben Karrieren.** Jeder Beruf, den der Spieler
  erreichen kann, existiert in der Sim.

## Bevölkerung

- Agenten: 100 → **300**.
- Messung (Seed 1, 100 Agenten, Release): 10 Jahre in 5,4 s, also ~36 s pro
  Leben (66 Jahre); mit 300 Agenten grob ~2 min.

## Berufe: zwei Schichten

1. **Gewerbe**: Berufe der Wertschöpfungsketten. Für **alle** Ketten gilt die
   generische Leiter **Gehilfe → Geselle → Meister → Betreiber** (Betreiber =
   Besitzer/Gründer). Voraussetzung: Level im Rang darunter plus ein Attribut.
   Gewerbe trainieren ein Attribut (Feld/Holz → Kraft, Schreibstube → Geist,
   Markt → Sozial, Weberei → Geschick). Sie sind damit das Trainingsfeld für die
   Pfade.
2. **Pfade**: die 16 Pfade mit 74 Berufen aus dem Setting. Selten,
   hierarchisch, mit Geheimnissen. Pfade sind Abnehmer der Ketten (Ritter
   brauchen Waffen und Pferde, Kirche Kerzen, Magier Bücher und Kräuter).

Spitzenämter sind Plätze (ein Bürgermeister, ein Bischof, ein Erzmagier pro
Turm) und von NPCs besetzt; der Spieler konkurriert um echte Plätze.

Mögliche spätere Seitenlinie: **Zunft** (Meister → Zunftmeister →
Gildenoberhaupt → „Meisterstück“), damit auch Gewerbe ein Geheimnis haben.

### Attribute und XP

- NPCs haben Attribute und Skills, als Zähler: XP pro Arbeitsblock, Level per
  Lookup in vorberechneten Schwellen.
- Freigeschaltete Berufe als Bitset pro Agent, neu berechnet nur bei Level-Up.
- Jobwechsel nur bei Level-Up, wöchentlich oder bei Kündigung.
- Lernraten: Tendenz gleiche Raten wie der Spieler (noch offen). Der Vorsprung
  des Spielers kommt aus Déjà-vu, Items, Wissen und Geheimnissen.
- Item-Multiplikatoren wirken **additiv** und **nur auf Job- und Skill-XP**,
  nicht auf Attribute.

## Wertschöpfungsketten

- Rezepte mit **höchstens 2 Inputs** (heute genau 1).
- **Rohstoffe begrenzt**, an Parzellen gebunden.
- **Außenwelt vorerst geschlossen** (kein Import/Export).
- Fest von Anfang an nur Nahrung und Bau; alles andere entsteht bei Nachfrage.

| Kette | Stufen | Abnehmer / Pfad |
|---|---|---|
| Brot (✔) | Feld → Mühle → Bäckerei | Hunger, Armenspeisung |
| Bier (✔) | Feld → Brauerei → Schenke | Komfort, Freizeit |
| Holz (✔) | Holzfäller → Sägewerk → Schreiner | Bau, Komfort |
| Ton (✔) | Tongrube → Ziegelei / Töpfer | Bau, Komfort |
| Textil (✔) | Schaf → Spinnerei → Weberei → Schneider | Komfort, Status |
| Käse (✔), Glas (✔) | wie heute | Komfort, Bau |
| Fleisch/Leder | Viehzucht → Metzger; Gerber → Sattler/Schuster | Hunger, Reiten, Ritter |
| Metall | Erzmine → Schmelze → Schmied / Waffenschmied | Wache, Söldner, Ritter |
| Pferde | Gestüt → Stall | Stallbursche, Reiten |
| Kerzen | Imker → Kerzenzieher | Kirche |
| Kräuter | Sammler → Tränkebrauer → Alchemist; Giftmischer | Gesundheit, Dunkle Magie |
| Papier | Weberei-Reste → Papiermühle → Buchbinder | Schreiber, Gelehrte, Magier |
| Gasthaus | Brot + Fleisch → Garküche | Item „Warme Mahlzeit“ |
| Bau | Maurer, Zimmermann (Dienst) | Wohnstufen |

Dienste ohne Lager: Diener, Schreiber, Bader, Spielmann, Lastenträger,
Laufbursche, Ausrufer.

### Parzellen

- Für den regulären Lauf fest in `world.toml`.
- Der **Parzellentyp ist Zustand, keine Konstante** (Wald, Feld, Mine,
  Kräuter, Kraftort, frei), mit Vorrat, der nachwächst oder sich erschöpft.
- Später: Aufforsten (Förster als Gewerbe), Zauber auf Parzellen, Abholzen –
  jeweils als Ereignis, damit deterministisch.

### Transport

- Nachschub braucht Tragestunden im Verhältnis zur Entfernung; Betriebe heuern
  Lastenträger als Tagelöhner an. Die Karte bekommt damit wirtschaftlichen Sinn.
- Einstiegsjobs laufen als **Tagelohn**: ohne Vertrag, pro Block bezahlt.

## Bedürfnisse

Jedes Bedürfnis hat Wachstum, Befriedigung und eine Folge, wenn es offen bleibt.

| Bedürfnis | Wächst durch | Befriedigt durch | Folge |
|---|---|---|---|
| Hunger (✔) | Zeit | Brot, Fleisch, Mahlzeit | Unterbrechung, Gesundheit |
| Müdigkeit (✔) | Wachzeit | Schlaf (besser im besseren Bett) | weniger Output |
| Sicherheit (✔) | fehlende Ersparnisse, Kriminalität | Ersparnisse, Wache, Schutzzauber | mehr Arbeit |
| **Gesundheit** (Zustand) | Alter, Hunger, Obdachlosigkeit, Kraft-Jobs, Krankheit | Bader, Tränke, Heilung | weniger Output, **kürzeres Leben** |
| **Glaube** (nur Fromme, neuer Trait) | Zeit | Messe, Spende, Kerzen | Unruhe; Fromme melden Ketzer |
| Komfort (✔) | Zeit | Möbel, Kleidung, … | Kaufdruck |
| **Status** (Soll − Ist) | Rang, Vermögen | Wohnstufe, Kleidung, Diener | Ämter verlangen Mindest-Status |
| **Freizeit** (ersetzt wander) | Arbeitszeit | Schenke, Turnier, Arena, Spaziergang | weniger Erholung |

Kosten: 7–8 Integer pro Agent, aktualisiert beim Aufwachen.

## Wohnstufen und Items

Die Stufe gehört zum Gebäude. Höhere Stufen brauchen mehr und edlere Baustoffe.
Jeder mit genug Geld darf jede Stufe bauen, auch einen Palast.

| Stufe | Status | Erholung | XP-Bonus | Unterhalt/Tag | Baustoffe |
|---|---|---|---|---|---|
| Straße | 0 | 50% | +0% | 0 | – |
| Strohlager | 1 | 75% | +0% | ~1 | Holz |
| Schlafsaal | 2 | 90% | +5% | ~3 | Bretter |
| Kammer | 3 | 100% | +10% | ~6 | Bretter, Ziegel |
| Bürgerhaus | 5 | 100% | +20% | ~15 | + Stein, Glas |
| Patrizierhaus | 7 | 110% | +35% | ~40 | + Möbel, Glas |
| Stadtpalais | 9 | 120% | +50% | ~100 | + Stein, Möbel, Kleidung |
| Palast | 10 | 120% | +75% | ~250 | alles, viel |

Zahlen sind Startwerte zum Balancieren.

| Item | Wie | Wirkung |
|---|---|---|
| Warme Mahlzeit | Abo bei Garküche/Schenke statt Brot | mehr Hunger-Entlastung, +Gesundheit, +10% XP |
| Diener | NPC mit Beruf Diener, Lohn vom Herrn | +Status, +10–30% XP je nach Diener-Level |

Die NPC-Wohnungswahl bekommt einen Term für die Status-Lücke.

## Bettler-Einstieg

Jede Stufe ist ein echter Job mit niedrigen Voraussetzungen, zahlt wenig,
reicht zum Überleben und trainiert ein Attribut.

- **Bettler**: Armenspeisung der Kirche (kauft Brot, begrenzt durch die
  Kirchenkasse) und Almosen von Passanten (Fromme oder mit Komfort-Überschuss,
  ein Wurf pro Aufwachen). Belebte Parzellen und Ortskenntnis bringen mehr.

| Attribut | Zwischenstufe | bezahlt von | Einstiegsberuf | bezahlt von |
|---|---|---|---|---|
| Kraft | Lastenträger | Betriebe (Transport) | Stallbursche | Gestüt, Stall |
| Sozial | Ausrufer | Stadt, Händler | Laufbursche | Reiche, Kaufleute |
| Geist | Kerzenjunge | Kerzenzieher, Kirche | Tempeldiener | Kirche |
| Geschick | Gassenjunge | Betteln | Taschendieb | Opfer |

## Institutionen

Institution = Kasse + Einnahmeregeln + Posten mit Lohn (Erweiterung der
heutigen Associations). Ist die Kasse leer, fallen Posten weg.

| Institution | Einnahmen | Zahlt |
|---|---|---|
| Stadt | Umsatz-, Gewinn-, Erbsteuer; Wegzoll; Mieten | Wache, Herold, Rat, Bürgermeister, Verwalter, Hofgelehrter, Kopfgeld |
| Kirche | Spenden, Zehnt, Gebühren (Taufe, Begräbnis) | Prediger, Priester, Inquisitor, Bischof, Almosen |
| Zünfte | Beiträge, Gründungsgebühr | Zunftmeister, Mindestpreis, Hilfe bei Pleite |
| Magierturm | Segen, Heilung, Artefakte; Stipendium der Stadt | Magier, Erzmagier; Novizen dienen |
| Diebesgilde | Anteil an der Beute, Hehlerei | Schutz, Bestechung |

## Magie (Low Fantasy)

- Magie ist selten und gefürchtet.
- Magie verkauft **befristete Modifikatoren**: Segen (+Output für Betriebe),
  Heilung, Schutzzauber (weniger Diebstahl), Wahrsagen; dazu Artefakte als Ware
  (aus Glas, Kräutern, Kerzen, Büchern). Segen gedeckelt und nicht stapelbar.
- Türme brauchen einen **Kraftort** (Parzelle); Arkanes Wissen lernt man nur
  mit Büchern.
- Magierturm: Novize → Magier → Erzmagier.
- „Das Echo“ wirkt nur für den Spieler.
- **Hexenmeister** mit Spielart **Nekromant** (über den Skill Okkultismus):
  Leichen vom Friedhof als Rohstoff, Totengespräch für Erben. Untote Arbeiter
  zurückgestellt. Ohne Okkultismus: Pakt und Gift.
- Flüche zurückgestellt (später ein weiterer Tat-Typ im Verdachtssystem).

## Verbrechen und Verfolgung

Ein gemeinsamer Mechanismus für alles Verbotene:

- Jede verbotene Tat erhöht einen **Verdacht**-Zähler pro Agent, der mit der
  Zeit sinkt.
- Jäger ermitteln wöchentlich; Chance aus Skill gegen Verdacht und Schleichen.
- Gefasst: Geld, Freiheit oder Steckbrief. Jäger bekommen Kopfgeld pro Fang.

| Täter | Jäger |
|---|---|
| Taschendieb, Dieb | Stadtwache |
| Wegelagerer | Söldner (Geleitschutz) |
| Giftmischer, Assassine | Ordnungshüter; Alchemist (Gegengift) |
| Raufbold | Stadtwache |
| Hexenmeister, Nekromant | Kirche: **Inquisitor** (Rang nach dem Priester, neben dem Bischof), Paladin |

Kopfgeldjäger: freier Jäger auf jeden Steckbrief.

## Einkommen und Grundversorgung

**Ziel: Ungleichheit ja, aber die Grundbedürfnisse müssen gedeckt sein.** Ein
Arbeiter in Vollzeit muss Essen und ein billiges Bett bezahlen können.

Ist-Zustand (Seed 1, 10 Jahre, aktuelle Config):

| Jahr | Beschäftigt | Lohn | Brot | Geld p10/p50/p90 | Gegründet/geschlossen |
|---|---|---|---|---|---|
| 1 | 84% | 5 | 20 | 173/264/340 | 20/14 |
| 3 | 79% | 5 | 17 | 214/280/320 | 30/24 |
| 10 | 75% | 5 | 19 | 88/189/333 | 151/140 |

Befunde:

1. Der Lohn liegt 10 Jahre lang am Minimum (`wage_min = 5`): mehr
   Arbeitskräfte als Stellen.
2. Arbeit allein trägt nicht (≈40 netto gegen ≈61 fürs Essen). Vermutlich
   schließen Transfers die Lücke (75% Gewinnsteuer, Erbsteuer, Umsatzsteuer als
   Fürsorge); Geldflüsse noch nicht geprüft.
3. Die Verteilung ist flach (p90 ≈ 2× Median).
4. 140 von 151 Neugründungen schließen wieder.

Stellschraube: Ein Brot kostet ~1,8 Arbeitsstunden, d. h. ~85% der
Arbeitszeit gehen ins Essen. **Was nach dem Essen an Arbeit frei ist, bestimmt,
wie viele Ketten die Stadt trägt.**

Geplante Richtung:

1. Grundversorgung produktiver machen: Ziel 35–40% der Arbeit für Nahrung
   (z. B. Feld 3/h, Mühle 4/h, Bäckerei 4/h → ~0,8 h pro Brot).
2. Gewinnsteuer senken (75% → 20–30%), damit Unternehmer reich werden.
3. Fürsorge ersetzen durch Armenspeisung und Almosen der Kirche mit
   begrenztem Budget.
4. Arbeitsnachfrage durch neue Ketten, Lastenträger und Diener; der Lohn löst
   sich dann vom Minimum.
5. Gründe für das Firmensterben untersuchen (Marge, Nachfrage, Kapital).

Absicherung: Kennzahl **Kaufkraft** = Nettolohn eines Gehilfen (10 h) /
(3,2 Mahlzeiten + billigstes Bett), in `periods.csv` und als CI-Grenze
(z. B. ≥ 1,1).

## Offen

- Lernraten der NPCs im Detail.
- Gründe für das Firmensterben.
- Reihenfolge der Umsetzung.
