-- Demo fixture: the "Baumhaus" (treehouse) building.
--
-- Idempotent: on re-run it first deletes the demo rows (children first),
-- then recreates them. Other data is left untouched.
--
-- Requires the schema from migrations 0001-0003; the app applies them
-- automatically on startup. See README ("Demo data") for loading steps.

PRAGMA foreign_keys = ON;

DELETE FROM tenancies WHERE apartment_id IN ('apartment-basement', 'apartment-ground', 'apartment-first', 'apartment-roof');
DELETE FROM ownerships WHERE apartment_id IN ('apartment-basement', 'apartment-ground', 'apartment-first', 'apartment-roof');
DELETE FROM apartments WHERE id IN ('apartment-basement', 'apartment-ground', 'apartment-first', 'apartment-roof');
DELETE FROM building_administrators WHERE building_id = 'building-treehouse';
DELETE FROM buildings WHERE id = 'building-treehouse';

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
INSERT INTO apartments (id, building_id, name, description, position)
VALUES
    ('apartment-basement', 'building-treehouse', 'Souterrain',    'Willies Souterrain-Refugium: Rasenmäher direkt vor der Tür, Dudelsack erst nach Feierabend.', 4),
    ('apartment-ground',   'building-treehouse', 'Erdgeschoß',    'Homers Parterre: Couch vor dem Fernseher und Donut-Duft im Treppenhaus.',                      3),
    ('apartment-first',    'building-treehouse', '1. Stock',      'Frau Krabappels Rückzugsort: hellhörig, aber leise – jede Störung wird mit einem „Ha!“ quittiert.', 2),
    ('apartment-roof',     'building-treehouse', 'Dachgeschoss',  'Barts Zimmer unterm Dach: Skateboard-Stellplatz, Klimaanlage und freie Sicht aufs ganze Viertel.', 1);

INSERT INTO ownerships (id, apartment_id, name, email, start_date)
VALUES
    ('ownership-macdougal', 'apartment-basement', 'Dr. William MacDougal III', 'willie.macdougal@example.com', '2026-01-01'),
    ('ownership-homer',     'apartment-ground',   'Homer Simpson',             'homer.simpson@example.com',    '2026-01-01'),
    ('ownership-krabappel', 'apartment-first',    'Edna Krabappel-Flanders',   'krabby@example.com',           '2026-01-01'),
    ('ownership-burns',     'apartment-roof',     'Charles Montgomery Burns',  'monty@example.com',            '2026-01-01');

INSERT INTO tenancies (id, apartment_id, name, email, start_date)
VALUES ('tenancy-bart-roof', 'apartment-roof', 'Bart Simpson', 'bart.simpson@example.com', '2026-01-01');
