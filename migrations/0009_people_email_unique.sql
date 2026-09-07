-- Person identity: the e-mail address is unique.
--
-- The application has always treated the e-mail as the person's identity
-- (resolve_person reuses an existing person with the same address instead of
-- creating a duplicate). This migration makes that rule a database invariant,
-- so an explicit person edit (the contact form on the person page) cannot
-- accidentally create two people with the same address either.
--
-- Comparison is case-insensitive and trimmed, like the identity matching in
-- the queries layer. The trigger only guards new writes; pre-existing
-- duplicates (which the application never created) are left untouched.

CREATE TRIGGER people_email_unique_insert BEFORE INSERT ON people
WHEN EXISTS (
    SELECT 1 FROM people
    WHERE lower(trim(email)) = lower(trim(NEW.email))
)
BEGIN
    SELECT RAISE(ABORT, 'Eine Person mit dieser E-Mail-Adresse existiert bereits.');
END;

CREATE TRIGGER people_email_unique_update BEFORE UPDATE ON people
WHEN EXISTS (
    SELECT 1 FROM people
    WHERE lower(trim(email)) = lower(trim(NEW.email))
      AND id != NEW.id
)
BEGIN
    SELECT RAISE(ABORT, 'Eine Person mit dieser E-Mail-Adresse existiert bereits.');
END;