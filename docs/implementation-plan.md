# Umsetzungsplan

Stand: 2026-11-20. Setzt [economy-design.md](economy-design.md) um.

## Regeln für jede Phase

- Jede Phase ist ein eigener Branch und PR und lässt die Sim lauffähig und
  deterministisch (CI-Job `determinism` über alle Plattformen grün).
- Jede Phase hat **Abnahmekriterien als Kennzahlen** in `periods.csv` bzw. CI,
  nicht nach Augenmaß.
- Neue Inhalte zuerst in `world.toml` und Schema; Logik nur, wo die
  Formelsprache nicht reicht.
- Golden-Logs (`sim/tests/golden`) werden bewusst neu erzeugt, wenn sich das
  Verhalten ändern soll.
- `core/src/world.rs` (~3800 Zeilen) wird nicht weiter aufgebläht: Neue
  Mechanismen bekommen eigene Module (`institution.rs`, `parcel.rs`,
  `attributes.rs`, `crime.rs`, `magic.rs`), die World ruft sie auf.
- Leistungsbudget: Arbeitszähler (Entscheidungen pro Agent und Tag, Optionen
  pro Entscheidung) dürfen pro Phase nur bewusst und dokumentiert steigen.

## Phase 0 – Messen

Voraussetzung für alles Weitere: Ohne Zahlen lässt sich nicht balancieren.

- Arbeitszähler und Laufzeit in CI (läuft bereits als eigene Session,
  Branch `claude/charming-jepsen-074f81`).
- Kennzahl **Kaufkraft** = Nettolohn Vollzeit / (3,2 Mahlzeiten + billigstes
  Bett) in `periods.csv`.
- **Geldflüsse** pro Periode in `periods.csv`: Löhne, Steuern je Art, Fürsorge,
  Renten, Erbe, Gründungskapital, Firmenschließungen.
- **Schließungsgründe** zählen (pleite, Besitzer tot, ohne Nachfrage, ohne
  Input).

Abnahme: Kennzahlen erscheinen in `periods.csv`, State-Hash unverändert.

## Phase 1 – Grundversorgung und Institutionen

Das Einkommensproblem lösen, bevor neue Ketten darauf aufbauen.

- Associations zu **Institutionen** verallgemeinern: Kasse, Einnahmeregeln,
  bezahlte Posten. Zuerst Stadt (aus Rat und heutiger Fürsorge) und Kirche.
- Fürsorge der Stadt ersetzen durch **Armenspeisung und Almosen der Kirche**
  mit begrenztem Budget (Kirche kauft Brot).
- Grundversorgung produktiver: Ziel 35–40% der Arbeit für Nahrung.
- Gewinnsteuer senken (75% → 20–30%).
- Firmensterben anhand der Schließungsgründe aus Phase 0 beheben.

Abnahme (Seed 1, 30 Jahre):
- Kaufkraft ≥ 1,1 im Median aller Perioden.
- Lohn liegt nicht dauerhaft am Minimum.
- p90/Median des Vermögens deutlich > 2 (Ungleichheit).
- Hungertote ≈ 0.

## Phase 2 – 300 Agenten, Parzellen

- Agenten 100 → 300; Karte, Häuser und Startbetriebe so, dass der Start trägt.
- **Parzellen als Zustand**: Typ (Wald, Feld, Mine, Kräuter, Kraftort, frei)
  und Vorrat mit Nachwachsen bzw. Erschöpfung. Rohstoffbetriebe brauchen eine
  passende Parzelle.
- Startbelegung fest in `world.toml`.

Abnahme: Kennzahlen aus Phase 1 halten bei 300 Agenten; ein Leben (66 Jahre)
in ≤ ~3 min Release.

## Phase 3 – Rezepte mit 2 Inputs, Transport, Tagelohn

- Rezepte mit bis zu 2 Inputs.
- Nachschub braucht **Tragestunden** nach Entfernung; Betriebe heuern
  Tagelöhner (Lastenträger) an.
- **Tagelohn** als Beschäftigungsform: ohne Vertrag, pro Block bezahlt.

Abnahme: Lastenträger sind dauerhaft beschäftigt; Standort wirkt messbar auf
Stückkosten.

## Phase 4 – Attribute und Gewerbe-Leiter

Ersetzt das heutige `career.rs`-Modell (ein Level pro Beruf).

- 4 Attribute (Kraft, Sozial, Geist, Geschick) und Skills als XP-Zähler mit
  Lookup-Level.
- **Generische Leiter** Gehilfe → Geselle → Meister → Betreiber für alle
  Betriebstypen; Voraussetzungen aus Rang und Attribut.
- Freigeschaltete Berufe als Bitset, neu berechnet nur bei Level-Up;
  Karriere-Entscheidung bei Level-Up, wöchentlich oder bei Kündigung.
- Traits greed/work_life prüfen: bleiben, ergänzt um Ehrgeiz/Fleiß falls
  nötig.

Abnahme: Arbeitszähler für Karriere ≤ ~1 Entscheidung pro Agent und Woche;
alle Ränge sind in 30 Jahren besetzt.

## Phase 5 – Bedürfnisse, Wohnstufen, Items

Status braucht Wohnstufen, darum zusammen.

- Neue Bedürfnisse: **Gesundheit** (Zustand, wirkt auf Lebensdauer),
  **Status** (Soll − Ist), **Freizeit** (ersetzt wander, mit Ort),
  **Glaube** (nur Fromme, neuer Trait).
- **Wohnstufen** pro Gebäude mit Status, Erholung, XP-Bonus, Unterhalt,
  stufenabhängigen Baustoffen; Wohnungswahl mit Status-Term.
- **Items**: Warme Mahlzeit, Diener (NPC-Beruf). XP-Boni additiv, nur auf
  Job- und Skill-XP.

Abnahme: alle Wohnstufen bis Patrizierhaus existieren in 30 Jahren; Diener
sind beschäftigt; Kaufkraft hält.

## Phase 6 – Neue Ketten

In dieser Reihenfolge, jede einzeln abnehmbar:

1. Fleisch/Leder (Viehzucht, Metzger, Gerber, Sattler/Schuster)
2. Garküche (Brot + Fleisch → Warme Mahlzeit)
3. Bau als Dienst (Maurer, Zimmermann)
4. Kerzen (Imker, Kerzenzieher)
5. Kräuter (Sammler, Tränkebrauer, Alchemist)
6. Papier (Papiermühle, Buchbinder)
7. Metall (Erzmine, Schmelze, Schmied, Waffenschmied)
8. Pferde (Gestüt, Stall)

Abnahme je Kette: entsteht emergent bei Nachfrage und hält sich in ≥ 50% der
Jahre nach ihrer ersten Gründung.

## Phase 7 – Bettler-Einstieg und erste Pfade

- Bettler (Almosen, Ortskenntnis), die 4 Zwischenstufen und 4 Einstiegsberufe.
- Erste Pfade mit Institutionen, die es schon gibt: **Kirche**
  (Prediger → Priester → Bischof/Inquisitor) und **Politik**
  (Herold → Ratsherr → Bürgermeister). Ämter verlangen Mindest-Status.

Abnahme: Ein NPC, der als Bettler startet, erreicht in einem Teil der Läufe
einen Einstiegsberuf und später ein Amt.

## Phase 8 – Verbrechen und Verfolgung

- Verdacht-Zähler, wöchentliche Ermittlung, Kopfgeld, Steckbrief.
- Taschendieb/Dieb gegen Stadtwache; Diebesgilde als Institution.
- Danach Wegelagerer, Raufbold, Assassine, Kopfgeldjäger.

Abnahme: Kriminalität und Wachstärke schwanken gegenläufig, statt in ein
Extrem zu kippen.

## Phase 9 – Magie

- Magierturm als Institution, Kraftort-Parzellen, Arkanes Wissen über Bücher.
- Befristete Modifikatoren (Segen gedeckelt, Heilung, Schutzzauber),
  Artefakte als Ware.
- Hexenmeister mit Spielart Nekromant; Inquisitor jagt ihn.

Abnahme: Segen wird nur gekauft, wenn er sich rechnet; Output-Effekt in Summe
begrenzt.

## Phase 10 – Restliche Pfade und Geheimnisse

- Übrige Hauptpfade und Seitenlinien mit Crossover-Bedingungen.
- Geheimnisse, Déjà-vu und Nebel auf Spielerseite.

## Abhängigkeiten

```
0 Messen
└─ 1 Grundversorgung, Institutionen
   └─ 2 300 Agenten, Parzellen
      └─ 3 2 Inputs, Transport, Tagelohn
         └─ 4 Attribute, Gewerbe-Leiter
            └─ 5 Bedürfnisse, Wohnstufen, Items
               ├─ 6 Neue Ketten
               └─ 7 Bettler, erste Pfade
                  ├─ 8 Verbrechen
                  └─ 9 Magie (braucht auch 6: Kräuter, Papier, Glas)
                     └─ 10 Restliche Pfade
```

Phase 6 und 7 sind nach Phase 5 parallel möglich, ebenso 8 und 9 nach 7.
