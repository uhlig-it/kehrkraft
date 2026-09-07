-- Demo fixture: three buildings.
--
--   1. "Baumhaus" – the treehouse (untouched from before).
--   2. "Elbphilharmonie" – fictional luxury flats in Hamburg with A-list
--      celebrity owners (with titles) and fancy Hamburg/Elbe apartment names;
--      about half of the flats are rented out to B-list celebrities.
--   3. "Haus 12, Am Platz der Jugend" – a fictional 11-story WBS-70 Plattenbau
--      in East Germany, owned entirely by one fictional housing company
--      (building-level owner, see migration 0005); every flat is rented to a
--      couple or individual with stereotypical East-German names.
--
-- Idempotent: on re-run it first deletes the demo rows, then recreates them.
-- Other data is left untouched.
--
-- Requires the schema from migrations 0001-0005; the app applies them
-- automatically on startup. See README ("Demo data") for loading steps.
--
-- The inserts run in one transaction: the ownerships FK is deferred and the
-- database requires an ownership row – or a covering building owner, see
-- migration 0005 – to exist before an apartment can be inserted
-- (`apartments_require_ownership` trigger), so ownerships and building owners
-- come first. The deletes go through the building's cascade, which removes
-- the ownerships, building owners and tenancies together with their
-- apartments (the delete guards of 0004/0005 only fire while the parent row
-- still exists, so they let the cascades pass).

PRAGMA foreign_keys = ON;

BEGIN;

DELETE FROM buildings WHERE id IN ('building-treehouse', 'building-elbphilharmonie', 'building-haus-12');

-- ---------------------------------------------------------------------------
-- 1. Baumhaus (treehouse), unchanged.
-- ---------------------------------------------------------------------------

-- Building with its administrator (contact): Bart Simpson
INSERT INTO buildings (id, name, description, secret_slug)
VALUES ('building-treehouse', 'Baumhaus', 'Barts Baumhaus hinter der 742 Evergreen Terrace – mit Klimaanlage und Aussicht auf Springfield.', 'LIwSIy5r0G4lSdwQwZbZbK');

INSERT INTO building_administrators (id, building_id, name, email)
VALUES ('admin-bart', 'building-treehouse', 'Bart Simpson', 'bart.simpson@example.com');

-- Apartments in their manual order (position 1-4, top floor first: Dach,
-- 1. Stock, Erdgeschoß, Souterrain), each with an open-ended ownership
-- starting 2026-01-01. The cleaning rotation follows the order the apartments
-- were created (all inserted in one statement, so the deterministic tie-break
-- is the id): basement, first, ground, roof. The roof floor is rented to Bart
-- Simpson, so the scheduler delegates roof duty to him.
INSERT INTO ownerships (id, apartment_id, name, email, start_date)
VALUES
    ('ownership-macdougal', 'apartment-basement', 'Dr. William MacDougal III', 'willie.macdougal@example.com', '2026-01-01'),
    ('ownership-homer',     'apartment-ground',   'Homer Simpson',             'homer.simpson@example.com',    '2026-01-01'),
    ('ownership-krabappel', 'apartment-first',    'Edna Krabappel-Flanders',   'krabby@example.com',           '2026-01-01'),
    ('ownership-burns',     'apartment-roof',     'Charles Montgomery Burns',  'monty@example.com',            '2026-01-01');

INSERT INTO apartments (id, building_id, name, description, position)
VALUES
    ('apartment-basement', 'building-treehouse', 'Souterrain',    'Willies Souterrain-Refugium: Rasenmäher direkt vor der Tür, Dudelsack erst nach Feierabend.', 4),
    ('apartment-ground',   'building-treehouse', 'Erdgeschoß',    'Homers Parterre: Couch vor dem Fernseher und Donut-Duft im Treppenhaus.',                      3),
    ('apartment-first',    'building-treehouse', '1. Stock',      'Frau Krabappels Rückzugsort: hellhörig, aber leise – jede Störung wird mit einem „Ha!“ quittiert.', 2),
    ('apartment-roof',     'building-treehouse', 'Dachgeschoss',  'Barts Zimmer unterm Dach: Skateboard-Stellplatz, Klimaanlage und freie Sicht aufs ganze Viertel.', 1);

INSERT INTO tenancies (id, apartment_id, name, email, start_date)
VALUES ('tenancy-bart-roof', 'apartment-roof', 'Bart Simpson', 'bart.simpson@example.com', '2026-01-01');

-- ---------------------------------------------------------------------------
-- 2. Elbphilharmonie
--
-- Fictional residents: A-list German celebrities (clearly invented, with
-- fancy titles) own the flats; four of the eight are rented out to invented
-- B-list celebrities. All ownerships started around the 2016/2017 opening
-- and are open-ended. The apartment names are invented in Hamburg/Elbe
-- marketing style (position 1 = top floor, the penthouse).
-- ---------------------------------------------------------------------------

-- Building with its administrator (contact): Hausverwaltung
INSERT INTO buildings (id, name, description, secret_slug)
VALUES ('building-elbphilharmonie', 'Elbphilharmonie', 'Die „Elphi“ an der Norderelbe: Konzerthaus, Hotel und exklusive Eigentumswohnungen über der Plaza – hier wohnt Hamburgs Prominenz hinter Glasfassade und Sicherheitsschleuse.', 'EDhvCHki1cnD_BeKzpbcCww');

INSERT INTO building_administrators (id, building_id, name, email)
VALUES ('admin-elphi-verwaltung', 'building-elbphilharmonie', 'Hausverwaltung Hafenkrone GmbH', 'verwaltung@hafenkrone.example');

INSERT INTO ownerships (id, apartment_id, name, email, start_date)
VALUES
    ('ownership-elphi-1', 'apartment-elphi-1', 'Prof. Dr. h.c. mult. Günter Klatsch', 'guenter.klatsch@example.com',    '2016-01-01'),
    ('ownership-elphi-2', 'apartment-elphi-2', 'Dr. Anita Alsterblick',              'anita.alsterblick@example.com',   '2016-01-01'),
    ('ownership-elphi-3', 'apartment-elphi-3', 'Udo Hafen, Dr. h.c.',                'udo.hafen@example.com',           '2017-01-01'),
    ('ownership-elphi-4', 'apartment-elphi-4', 'Prof. Dr. Knut Nebel',               'knut.nebel@example.com',          '2017-01-01'),
    ('ownership-elphi-5', 'apartment-elphi-5', 'Dr. Bastian Brandung',               'bastian.brandung@example.com',    '2016-01-01'),
    ('ownership-elphi-6', 'apartment-elphi-6', 'Univ.-Prof. Dr. Elke Sturmflut',     'elke.sturmflut@example.com',      '2017-01-01'),
    ('ownership-elphi-7', 'apartment-elphi-7', 'Sönke Sandbank, Dr. med. h.c.',      'soenke.sandbank@example.com',     '2016-01-01'),
    ('ownership-elphi-8', 'apartment-elphi-8', 'Marlene Möwe, Prof. h.c.',           'marlene.moewe@example.com',       '2017-01-01');

INSERT INTO apartments (id, building_id, name, description, position)
VALUES
    ('apartment-elphi-1', 'building-elbphilharmonie', 'Gläserne Welle',    'Penthouse im 19. Obergeschoss: Glasfassade, Ellbogenfreiheit und eine Küche, in der nur Gäste kochen dürfen. Blick über die Norderelbe bis zum Michel.', 1),
    ('apartment-elphi-2', 'building-elbphilharmonie', 'Elbblick-Suite',    'Suite im 18. Obergeschoss: zwei Schlafzimmer, ein Flügel im Wohnzimmer und ein Balkon, der bei Sturmflut zum Sperrgebiet erklärt wird.',                2),
    ('apartment-elphi-3', 'building-elbphilharmonie', 'Hafenkrone-Loft',   'Loft im 16. Obergeschoss: sechs Meter hohe Decken, roher Beton und eine Fensterfront, die die Hafenkräne live ins Wohnzimmer projiziert.',             3),
    ('apartment-elphi-4', 'building-elbphilharmonie', 'Norderelbe-Panorama', 'Panorama-Wohnung im 14. Obergeschoss: Fensterfront Richtung Containerterminal, abends kostenlos das Lichtermeer der Docks.',                          4),
    ('apartment-elphi-5', 'building-elbphilharmonie', 'Kaispeicher-Atelier', 'Atelier im 12. Obergeschoss: Nordlicht, Eichenparkett und ein Blick auf die Kräne und Backsteinfassaden der Speicherstadt.',                           5),
    ('apartment-elphi-6', 'building-elbphilharmonie', 'Sturmflut-Suite',   'Suite im 11. Obergeschoss: schalldichte Fenster zur Elbe, begehbarer Kleiderschrank und ein Notvorrat für drei Tage Landunter.',                       6),
    ('apartment-elphi-7', 'building-elbphilharmonie', 'Möwenflug',         'Kompakte Wohnung im 10. Obergeschoss: Arbeitszimmer mit Elbblick, Gästezimmer für den Pressesprecher, Balkon zuerst für die Möwen.',                   7),
    ('apartment-elphi-8', 'building-elbphilharmonie', 'Plaza-Blick',       'Im 9. Obergeschoss direkt über der Plaza: abends Konzertbesucher als Vorgarten, morgens Ruhe und Espresso auf der Terrasse.',                            8);

-- Rented out to invented B-list celebrities (about half of the flats).
INSERT INTO tenancies (id, apartment_id, name, email, start_date)
VALUES
    ('tenancy-elphi-2', 'apartment-elphi-2', 'Sandra Sunshine',      'sandra.sunshine@example.com',      '2019-05-01'),
    ('tenancy-elphi-4', 'apartment-elphi-4', 'Kevin Feuerstein',     'kevin.feuerstein@example.com',     '2021-02-01'),
    ('tenancy-elphi-7', 'apartment-elphi-7', 'Ronny Peppermint',     'ronny.peppermint@example.com',     '2022-11-01'),
    ('tenancy-elphi-8', 'apartment-elphi-8', 'Jacqueline Sterni',    'jacqueline.sterni@example.com',    '2018-09-01');

-- ---------------------------------------------------------------------------
-- 3. Haus 12, Am Platz der Jugend
--
-- Fictional WBS-70 Plattenbau, 11 stories (Erdgeschoss + 10 Obergeschosse),
-- built 1986 in a fictional East-German settlement. The whole building is
-- owned by one entity – the fictional housing company "Deutsche Wohnbau SE"
-- – since 1995 (Treuhand privatization), so the building needs no
-- per-apartment ownership records (building_owners row, migration 0005).
-- Every flat is rented to a couple or individual with stereotypical
-- East-German names; historic, open-ended tenancies.
--
-- Rotation order follows the id tie-break (single insert statement):
-- Erdgeschoss first, then 1. OG … 10. OG (bottom-up). Positions are manual
-- display order, top floor first (10. OG = 1 … EG = 33).
-- ---------------------------------------------------------------------------

INSERT INTO buildings (id, name, description, secret_slug)
VALUES ('building-haus-12', 'Haus 12, Am Platz der Jugend', 'WBS-70-Plattenbau mit 11 Geschossen, errichtet 1986 am Platz der Jugend in einer fiktiven ostdeutschen Plattenbausiedlung: Balkone gen Osten, Blick auf Garagenhof und Konsum. Das ganze Haus gehört der Deutschen Wohnbau SE – und jeder Mieter kennt seine Kehrwoche.', 'G4h0ev2OwIPFjf-eT_WHoww');

INSERT INTO building_administrators (id, building_id, name, email)
VALUES ('admin-haus-12-dw', 'building-haus-12', 'Doreen Ludwig, Objektbetreuung Deutsche Wohnbau SE', 'doreen.ludwig@deutsche-wohnbau.example');

-- The one owner of the whole building; covers today and the future.
INSERT INTO building_owners (id, building_id, name, email, start_date)
VALUES ('building-owner-haus-12-dw', 'building-haus-12', 'Deutsche Wohnbau SE', 'service@deutsche-wohnbau.example', '1995-01-01');

INSERT INTO apartments (id, building_id, name, description, position)
VALUES
    ('apartment-hh-00-links',   'building-haus-12', 'Erdgeschoss, links',  'Erdgeschoss mit Blick auf die gepflegten Beete der Rentner-Kommission: zwei Zimmer, ein Kellerabteil und der Geruch von Bohnerwachs im Treppenhaus.', 33),
    ('apartment-hh-00-mitte',   'building-haus-12', 'Erdgeschoss, Mitte',  'Die gute Stube neben dem Treppenhaus: Flur mit Schuhschrank, Sonntagsbraten beim Bäcker um die Ecke und eine Heizung, die den Winter verschlafen hat.', 32),
    ('apartment-hh-00-rechts',  'building-haus-12', 'Erdgeschoss, rechts', 'Zwei Zimmer im Hochparterre: vom Balkon sieht man die Bushaltestelle – und die Haltestelle sieht zurück. Spreewaldgurken vom Konsum lagern im Keller.', 31),
    ('apartment-hh-01-links',   'building-haus-12', '1. OG, links',        'Zwei Zimmer plus Abstellraum: Balkon gen Osten, Geranien inklusive, und pünktlich um zwölf zieht der Geruch von Frikadellen durchs Haus.', 30),
    ('apartment-hh-01-mitte',   'building-haus-12', '1. OG, Mitte',        'Wohnung mit Westblick: abends scheint die Sonne in die Küche, morgens pfeift der Boiler das Lied vom warmen Wasser.', 29),
    ('apartment-hh-01-rechts',  'building-haus-12', '1. OG, rechts',       'Die Wohnung mit der schönsten Tapete des Hauses: Blümchenmuster von 1987, vom Vormieter sorgsam gepflegt und beim Auszug als Andenken hinterlassen.', 28),
    ('apartment-hh-02-links',   'building-haus-12', '2. OG, links',        'Drei Zimmer in WBS-70-Manier: gerade Wände, gerade Decken und ein Balkon, auf dem die Klappliege Platz hat, ohne anzustoßen.', 27),
    ('apartment-hh-02-mitte',   'building-haus-12', '2. OG, Mitte',        'Familienwohnung mit Südloggia: Platz für Schrankwand, Bücherregal und das Standfahrrad des Sohnes – die Loggia hat schon viele Nachbarschaftsfeiern überlebt.', 26),
    ('apartment-hh-02-rechts',  'building-haus-12', '2. OG, rechts',       'Zwei Zimmer, Bad, Balkon: von hier aus kann man dem Imbiss-Brötchen beim Verkauf zusehen und dem Milchwagen beim Klingeln zuhören.', 25),
    ('apartment-hh-03-links',   'building-haus-12', '3. OG, links',        'Die Wohnung über den Garagen: morgens Trabant-Geknatter als Wecker, abends Motorenöl und Freiheit in der Luft.', 24),
    ('apartment-hh-03-mitte',   'building-haus-12', '3. OG, Mitte',        'Heimelige Drei-Zimmer-Wohnung: die Wanduhr tickt im Takt der Heizung, und der Teppich kennt den Weg zum Fernseher auswendig.', 23),
    ('apartment-hh-03-rechts',  'building-haus-12', '3. OG, rechts',       'Mit Balkon zur Straße: beste Aussicht auf die LPG-Erntefahrzeuge, den Trockenplatz und Herrn Wuschek mit seinem Dackel.', 22),
    ('apartment-hh-04-links',   'building-haus-12', '4. OG, links',        'Zwei Zimmer mit Ostbalkon: Blick über die Dächer der Siedlung, den Garagenhof und den Schuppen von Familie Krüger.', 21),
    ('apartment-hh-04-mitte',   'building-haus-12', '4. OG, Mitte',        'Ruhige Wohnung zum Hof: hier wohnt man über dem Trockenplatz und unter der Nachtigall – ein Plattenbau-Märchen.', 20),
    ('apartment-hh-04-rechts',  'building-haus-12', '4. OG, rechts',       'Drei Zimmer mit Südloggia und Wandschrank: der Wandschrank hat drei Umzüge überlebt, die Loggia alle Nachbarschaftsfeiern.', 19),
    ('apartment-hh-05-links',   'building-haus-12', '5. OG, links',        'Oben angekommen – fast: Blick über die Siedlung bis zum Wasserturm, Briefkasten mit Eigenbau-Klingelschild und ein Flur in Eichenoptik.', 18),
    ('apartment-hh-05-mitte',   'building-haus-12', '5. OG, Mitte',        'Wohnung für Individualisten: Flur mit Bücherregal, Küche mit Eierkocher und ein Balkon, auf dem die Geranien nur auf Antrag wachsen.', 17),
    ('apartment-hh-05-rechts',  'building-haus-12', '5. OG, rechts',       'Zwei Zimmer, ein Bad, ein Balkon gen Osten – und die Hausordnung hängt über dem Esstisch, falls jemand die Kehrwoche vergisst.', 16),
    ('apartment-hh-06-links',   'building-haus-12', '6. OG, links',        'Drittes Obergeschoss mit Fernblick: an klaren Tagen sieht man den Fahnenmast der Kreisverwaltung und das Umspannwerk dahinter.', 15),
    ('apartment-hh-06-mitte',   'building-haus-12', '6. OG, Mitte',        'Die Wohnung mit dem hellsten Bad des Hauses: Keramik von 1989, frisch gefliest und nach Chlor duftend.', 14),
    ('apartment-hh-06-rechts',  'building-haus-12', '6. OG, rechts',       'Zwei Zimmer mit Balkon Richtung Kita: morgens Kinderlachen, mittags Mittagsruhe, abends Elternabende – das Leben in der Platte eben.', 13),
    ('apartment-hh-07-links',   'building-haus-12', '7. OG, links',        'Höhenlage mit Aussicht auf den Konsum-Kühlwagen: der Einkauf ist von hier aus schneller beobachtet als erledigt.', 12),
    ('apartment-hh-07-mitte',   'building-haus-12', '7. OG, Mitte',        'Vier Zimmer für die ganze Familie: Wandverkleidung, Abstellraum und eine Heizung, die nachts leise vor sich hin kocht.', 11),
    ('apartment-hh-07-rechts',  'building-haus-12', '7. OG, rechts',       'Zwei Zimmer, Balkon gen Osten: die Aussicht auf den Garagenhof wird mit dem Geruch von Bratkartoffeln aus der Nebenwohnung geliefert.', 10),
    ('apartment-hh-08-links',   'building-haus-12', '8. OG, links',        'Drei Zimmer über den Dächern: man sieht die Platte nebenan, das Neubaugebiet dahinter und – bei gutem Wetter – das Ende der Stadt.', 9),
    ('apartment-hh-08-mitte',   'building-haus-12', '8. OG, Mitte',        'Sonnenwohnung mit Loggia: die Sonne hat morgens ihren Auftritt, mittags Pause und abends Feierabend – die Miete bleibt gleich.', 8),
    ('apartment-hh-08-rechts',  'building-haus-12', '8. OG, rechts',       'Zwei Zimmer mit Südwestbalkon: abends glüht der Himmel über dem Kraftwerk, und die Laternen gehen im Takt der Siedlung an.', 7),
    ('apartment-hh-09-links',   'building-haus-12', '9. OG, links',        'Fast Hochhaus-Atmosphäre: der Aufzug hält nicht immer, aber wer hier wohnt, hat ohnehin die bessere Sicht auf die LPG-Felder.', 6),
    ('apartment-hh-09-mitte',   'building-haus-12', '9. OG, Mitte',        'Ruhige Wohnung unter dem Flachdach: leise Heizung, stabile Wände, und der Postbote kennt die Bewohner mit Namen.', 5),
    ('apartment-hh-09-rechts',  'building-haus-12', '9. OG, rechts',       'Drei Zimmer mit Fernblick: vom Balkon aus kann man dem Zug beim Vorbeifahren und der Ringstraße beim Zubetonieren zusehen.', 4),
    ('apartment-hh-10-links',   'building-haus-12', '10. OG, links',       'Die Krönung: oberstes Geschoss mit Blick über die ganze Siedlung, den Wasserturm und, im Sommer, die flirrende Hitze über dem Asphalt.', 1),
    ('apartment-hh-10-mitte',   'building-haus-12', '10. OG, Mitte',       'Oberste Wohnung, beste Aussicht: zwei Zimmer, zwei Balkone und ein Fernblick, für den anderswo Miete fällig wäre.', 2),
    ('apartment-hh-10-rechts',  'building-haus-12', '10. OG, rechts',      'Zehnter Stock, Südseite: hier wacht die Sonne zuerst auf, und der Fernblick reicht bis zum Hochsitz des Jägervereins.', 3);

INSERT INTO tenancies (id, apartment_id, name, email, start_date)
VALUES
    ('tenancy-hh-00-links',   'apartment-hh-00-links',   'Ronny & Mandy Schiller',    'ronny.schiller@example.com',     '1996-03-15'),
    ('tenancy-hh-00-mitte',   'apartment-hh-00-mitte',   'Maik Giese',                 'maik.giese@example.com',         '2001-08-01'),
    ('tenancy-hh-00-rechts',  'apartment-hh-00-rechts',  'Doreen & Frank Krüger',      'doreen.krueger@example.com',     '1997-01-10'),
    ('tenancy-hh-01-links',   'apartment-hh-01-links',   'Sandy Lehmann',              'sandy.lehmann@example.com',      '2003-05-12'),
    ('tenancy-hh-01-mitte',   'apartment-hh-01-mitte',   'Jacqueline Wünsche',         'jacqueline.wuensche@example.com','2010-09-01'),
    ('tenancy-hh-01-rechts',  'apartment-hh-01-rechts',  'Ramona & Uwe Henke',         'ramona.henke@example.com',       '1995-11-02'),
    ('tenancy-hh-02-links',   'apartment-hh-02-links',   'Kati Marquardt',             'kati.marquardt@example.com',     '2008-04-21'),
    ('tenancy-hh-02-mitte',   'apartment-hh-02-mitte',   'Silvio & Steffi Neumann',    'silvio.neumann@example.com',     '1999-07-16'),
    ('tenancy-hh-02-rechts',  'apartment-hh-02-rechts',  'Bärbel Noack',               'baerbel.noack@example.com',      '2006-12-01'),
    ('tenancy-hh-03-links',   'apartment-hh-03-links',   'Torsten Kroll',              'torsten.kroll@example.com',      '2013-06-17'),
    ('tenancy-hh-03-mitte',   'apartment-hh-03-mitte',   'Mirko & Antje Vogler',       'mirko.vogler@example.com',       '1998-03-09'),
    ('tenancy-hh-03-rechts',  'apartment-hh-03-rechts',  'Grit Schulze',               'grit.schulze@example.com',       '2000-10-23'),
    ('tenancy-hh-04-links',   'apartment-hh-04-links',   'Jörg & Petra Lenz',          'joerg.lenz@example.com',         '2004-02-02'),
    ('tenancy-hh-04-mitte',   'apartment-hh-04-mitte',   'Nadine & Mario Wolf',        'nadine.wolf@example.com',        '2012-08-27'),
    ('tenancy-hh-04-rechts',  'apartment-hh-04-rechts',  'Holger Seifert',             'holger.seifert@example.com',     '1996-09-30'),
    ('tenancy-hh-05-links',   'apartment-hh-05-links',   'Ingo & Heike Brandt',        'ingo.brandt@example.com',        '2007-03-05'),
    ('tenancy-hh-05-mitte',   'apartment-hh-05-mitte',   'Manuela Richter',            'manuela.richter@example.com',    '2014-11-11'),
    ('tenancy-hh-05-rechts',  'apartment-hh-05-rechts',  'Sven & Katrin Winter',       'sven.winter@example.com',        '2002-05-13'),
    ('tenancy-hh-06-links',   'apartment-hh-06-links',   'Rico & Doreen Otto',         'rico.otto@example.com',          '1997-08-18'),
    ('tenancy-hh-06-mitte',   'apartment-hh-06-mitte',   'Steffi Krause',              'steffi.krause@example.com',      '2005-01-31'),
    ('tenancy-hh-06-rechts',  'apartment-hh-06-rechts',  'Karsten & Sylke Pohl',       'karsten.pohl@example.com',       '2009-07-07'),
    ('tenancy-hh-07-links',   'apartment-hh-07-links',   'Mandy Böhme',                'mandy.boehme@example.com',       '2016-04-04'),
    ('tenancy-hh-07-mitte',   'apartment-hh-07-mitte',   'Maik & Ramona Jäger',        'maik.jaeger@example.com',        '2001-12-03'),
    ('tenancy-hh-07-rechts',  'apartment-hh-07-rechts',  'Peggy Werner',               'peggy.werner@example.com',       '2011-10-10'),
    ('tenancy-hh-08-links',   'apartment-hh-08-links',   'René & Birgit Lorenz',       'rene.lorenz@example.com',        '1998-06-22'),
    ('tenancy-hh-08-mitte',   'apartment-hh-08-mitte',   'Cindy Schubert',             'cindy.schubert@example.com',     '2015-03-16'),
    ('tenancy-hh-08-rechts',  'apartment-hh-08-rechts',  'Mario & Manuela Hartmann',   'mario.hartmann@example.com',     '2003-09-29'),
    ('tenancy-hh-09-links',   'apartment-hh-09-links',   'Antje Fischer',              'antje.fischer@example.com',      '2007-11-26'),
    ('tenancy-hh-09-mitte',   'apartment-hh-09-mitte',   'Jens & Peggy Wendt',         'jens.wendt@example.com',         '1996-01-08'),
    ('tenancy-hh-09-rechts',  'apartment-hh-09-rechts',  'Sylke Hartwig',              'sylke.hartwig@example.com',      '2012-02-14'),
    ('tenancy-hh-10-links',   'apartment-hh-10-links',   'Ronny Lehmann',              'ronny.lehmann@example.com',      '2008-08-08'),
    ('tenancy-hh-10-mitte',   'apartment-hh-10-mitte',   'Katrin & Jörg Schönfeld',    'katrin.schoenfeld@example.com',  '2000-04-25'),
    ('tenancy-hh-10-rechts',  'apartment-hh-10-rechts',  'Steffi & Mirko Bastian',     'steffi.bastian@example.com',     '2017-05-22');

COMMIT;
