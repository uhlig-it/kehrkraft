//! Lightweight translation ("i18n") for the Kehrkraft UI.
//!
//! The app speaks German and English. German stays the fallback language
//! (the app's historical default), but once a visitor picks a language the
//! choice wins over their browser preference: the language selector sets a
//! cookie (`lang=de` / `lang=en`) that overrides `Accept-Language` from then
//! on. Later this preference will move into the user profile.
//!
//! All user-facing copy lives in one catalog (`MESSAGES`): every message has
//! a stable key plus a German and an English rendering. Templates reach it
//! through a `Ui` value (one per request), Rust code through [`msg`] /
//! [`msgf`]. Messages may contain `{0}`/`{1}`/… placeholders; the `t1`–`t3`
//! methods on [`Ui`] and [`msgf`] substitute them.
//!
//! The SQLite validation triggers raise stable `ERR_*` codes (see migration
//! 0012) instead of display text; the codes are ordinary catalog keys, so the
//! database stays language-neutral and the UI translates at render time.

use std::fmt::Write as _;

/// The languages the UI can be rendered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

impl Lang {
    /// Both supported languages, German first (the fallback order).
    pub const ALL: [Lang; 2] = [Lang::De, Lang::En];

    /// The language code used in URLs, cookies and the `lang` attribute.
    pub fn code(self) -> &'static str {
        match self {
            Lang::De => "de",
            Lang::En => "en",
        }
    }

    /// Parse a `de`/`en` language code (used for the cookie and the
    /// `/language/{code}` route).
    pub fn from_code(code: &str) -> Option<Lang> {
        match code {
            "de" => Some(Lang::De),
            "en" => Some(Lang::En),
            _ => None,
        }
    }
}

/// The name of the language-preference cookie.
pub const COOKIE_NAME: &str = "lang";

/// Lifetime of the preference cookie ("wins from now on" until changed).
pub const COOKIE_MAX_AGE: u32 = 60 * 60 * 24 * 365;

/// Read a cookie by name from a raw `Cookie` request header.
pub fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

/// Pick the UI language for a request: an explicit `lang` cookie wins,
/// otherwise the browser's `Accept-Language` list decides, and the app's
/// historical default (German) is the fallback when the browser names no
/// supported language.
pub fn negotiate(cookie: Option<&str>, accept_language: Option<&str>) -> Lang {
    if let Some(lang) = cookie.map(str::trim).and_then(Lang::from_code) {
        return lang;
    }
    accept_language.and_then(best_match).unwrap_or(Lang::De)
}

/// The first language of an `Accept-Language` header that we support,
/// honouring `q` weights (RFC 7231 §5.3.1); `None` if the browser prefers no
/// supported language.
fn best_match(accept_language: &str) -> Option<Lang> {
    let mut entries: Vec<(&str, f32)> = accept_language
        .split(',')
        .filter_map(|entry| {
            let mut parts = entry.split(';');
            let tag = parts.next()?.trim();
            if tag.is_empty() {
                return None;
            }
            // The q weight defaults to 1; "de-DE" and friends match on their
            // primary language tag.
            let q = parts
                .next()
                .and_then(|p| p.trim().strip_prefix("q="))
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(1.0);
            Some((tag, q))
        })
        .collect();
    entries.sort_by(|a, b| b.1.total_cmp(&a.1));
    entries.into_iter().find_map(|(tag, q)| {
        if q <= 0.0 {
            return None;
        }
        let primary = tag.split('-').next().unwrap_or(tag);
        Lang::from_code(primary)
    })
}

/// The `Ui` value handed to every template so its copy can be rendered in
/// the request's language: `{{ ui.t("key") }}` prints a plain message,
/// `{{ ui.t2("key", &a, &b) }}` one with `{0}`/`{1}` placeholders.
#[derive(Debug, Clone, Copy)]
pub struct Ui {
    pub lang: Lang,
}

impl Ui {
    pub fn for_lang(lang: Lang) -> Ui {
        Ui { lang }
    }

    /// HTML `lang` attribute value ("de"/"en").
    pub fn lang_code(&self) -> &'static str {
        self.lang.code()
    }

    /// Whether the German UI is active (used to highlight the selector).
    pub fn is_de(&self) -> bool {
        self.lang == Lang::De
    }

    /// A message without placeholders.
    pub fn t(&self, key: &'static str) -> &'static str {
        msg(self.lang, key)
    }

    /// A message with one `{0}` placeholder.
    pub fn t1(&self, key: &'static str, a: &str) -> String {
        msgf(self.lang, key, &[a])
    }

    /// A message with `{0}` and `{1}` placeholders.
    pub fn t2(&self, key: &'static str, a: &str, b: &str) -> String {
        msgf(self.lang, key, &[a, b])
    }

    /// A message with `{0}`, `{1}` and `{2}` placeholders.
    pub fn t3(&self, key: &'static str, a: &str, b: &str, c: &str) -> String {
        msgf(self.lang, key, &[a, b, c])
    }
}

/// One catalog entry: stable key, German text, English text. The German
/// texts mirror the pre-i18n UI verbatim (the fallback language and the
/// baseline the e2e tests assert).
type Entry = (&'static str, &'static str, &'static str);

/// The whole catalog, grouped by page. Keys are stable identifiers; the
/// `ERR_*` keys double as the codes the database triggers raise.
static MESSAGES: &[Entry] = &[
    // --- Layout (base.html) ---
    ("nav.buildings", "Gebäude", "Buildings"),
    ("nav.people", "Personen", "People"),
    ("brand.alt", "Kehrkraft-Logo", "Kehrkraft logo"),
    ("lang.switch", "Sprache wählen", "Choose language"),
    (
        "footer.tagline",
        "Kehrkraft · Kehrwoche-Rotationsplan",
        "Kehrkraft · Kehrwoche rotation schedule",
    ),

    // --- Shared bits of the forms and tables ---
    ("form.name_max", "(max. 30 Zeichen)", "(max. 30 characters)"),
    ("form.optional", "(optional)", "(optional)"),
    ("form.description", "Beschreibung", "Description"),
    ("form.email", "E-Mail", "E-mail"),
    ("form.ownership_from", "Eigentum ab", "Ownership from"),
    ("common.cancel", "Abbrechen", "Cancel"),
    ("common.edit", "Bearbeiten", "Edit"),
    ("common.delete", "Löschen", "Delete"),
    ("common.save_changes", "Änderungen speichern", "Save changes"),
    ("common.yes_end_anyway", "Ja, dennoch beenden", "Yes, end it anyway"),
    ("common.no_dont_end", "Nein, nicht beenden", "No, don't end it"),
    ("common.not_found", "Nicht gefunden", "Not found"),
    ("table.email", "E-Mail", "E-mail"),
    ("table.start", "Beginn", "Start"),
    ("table.end", "Ende", "End"),
    (
        "suggestions.help",
        "Bekannte Personen erscheinen bei der Eingabe als Vorschlag; dieselbe E-Mail-Adresse bedeutet dieselbe Person.",
        "Known people appear as suggestions while you type; the same e-mail address always means the same person.",
    ),

    // --- Buildings index ---
    ("buildings.heading", "Gebäude", "Buildings"),
    ("buildings.new", "Neues Gebäude", "New building"),
    ("buildings.empty", "Noch keine Gebäude.", "No buildings yet."),
    (
        "buildings.empty_cta",
        "Lege dein erstes Gebäude an",
        "Create your first building",
    ),
    (
        "buildings.delete_confirm",
        "Dieses Gebäude samt aller Wohnungen löschen?",
        "Delete this building and all of its apartments?",
    ),

    // --- Building form (new/edit) ---
    ("buildings.new_title", "Neues Gebäude anlegen", "Create a new building"),
    ("buildings.edit_title", "Gebäude bearbeiten", "Edit building"),
    ("buildings.contact_label", "Ansprechpartner", "Contact person"),
    (
        "buildings.new_contact_help",
        "Beide Felder leer lassen, wenn es keinen Ansprechpartner geben soll. Bekannte Personen erscheinen bei der Eingabe als Vorschlag; dieselbe E-Mail-Adresse bedeutet dieselbe Person.",
        "Leave both fields empty when there is no contact person. Known people appear as suggestions while you type; the same e-mail address always means the same person.",
    ),
    (
        "buildings.edit_contact_help",
        "Beide Felder leer lassen, um den bisherigen Ansprechpartner zu behalten. Bekannte Personen erscheinen bei der Eingabe als Vorschlag; dieselbe E-Mail-Adresse bedeutet dieselbe Person.",
        "Leave both fields empty to keep the current contact person. Known people appear as suggestions while you type; the same e-mail address always means the same person.",
    ),
    ("buildings.ownership_label", "Eigentumsverhältnisse", "Ownership structure"),
    (
        "buildings.ownership_help",
        "Entscheiden Sie sich für eine der beiden Formen – die Aufteilung eines von einer Person besessenen Gebäudes in einzelne Eigentumswohnungen (WEG) wird von Kehrkraft nicht unterstützt.",
        "Choose one of the two forms – splitting a building owned by a single person into individually owned apartments (WEG) is not supported by Kehrkraft.",
    ),
    (
        "buildings.style_apartments",
        "Einzelne Wohnungen: Jede Wohnung gehört einer eigenen Person oder Gesellschaft (WEG). Beim Anlegen jeder Wohnung ist ihr erster Eigentümer anzugeben.",
        "Individual apartments: each apartment belongs to its own person or company (WEG). When creating an apartment, its first owner must be provided.",
    ),
    (
        "buildings.style_building",
        "Ein Gebäudeeigentümer: Eine Person oder Gesellschaft besitzt das ganze Gebäude (z. B. eine Wohnungsgesellschaft). Die Wohnungen gehören dann mit zum Gebäude – einzelne Wohnungseigentümer gibt es nicht.",
        "One building owner: a single person or company owns the whole building (e.g. a housing company). The apartments then come with the building – there are no individual apartment owners.",
    ),
    (
        "buildings.owner_help",
        "Wer ist der Gebäudeeigentümer? Bekannte Personen erscheinen bei der Eingabe als Vorschlag; dieselbe E-Mail-Adresse bedeutet dieselbe Person.",
        "Who is the building owner? Known people appear as suggestions while you type; the same e-mail address always means the same person.",
    ),
    ("buildings.create", "Gebäude anlegen", "Create building"),
    ("buildings.save", "Gebäude speichern", "Save building"),
    (
        "buildings.owner_required",
        "Für ein Gebäude mit einem einzigen Eigentümer müssen Name, E-Mail-Adresse und Beginn („Eigentum ab“) angegeben werden.",
        "For a building with a single owner, the name, e-mail address and start date (“Ownership from”) must be provided.",
    ),

    // --- Building page ---
    ("buildings.contact_label_line", "Ansprechpartner:", "Contact person:"),
    (
        "buildings.public_pdf",
        "Plan als öffentliches PDF",
        "Schedule as public PDF",
    ),
    ("buildings.public_ical", "öffentlicher iCal-Feed", "Public iCal feed"),
    ("buildings.schedule_heading", "Kehrwochen-Plan", "Kehrwoche schedule"),
    (
        "buildings.full_year",
        "Kompletter Jahresplan",
        "Full-year schedule",
    ),
    (
        "buildings.no_upcoming",
        "Für dieses Jahr sind keine kommenden Wochen mehr eingeplant.",
        "No upcoming weeks of this year remain scheduled.",
    ),
    ("schedule.week", "KW", "CW"),
    ("schedule.from", "von", "From"),
    ("schedule.to", "bis", "To"),
    ("schedule.tenant", "Mieter", "Tenant"),
    ("schedule.unassigned", "Nicht zugewiesen", "Unassigned"),
    ("buildings.apartments_heading", "Wohnungen", "Apartments"),
    ("buildings.saving_order", "Speichere Reihenfolge …", "Saving order …"),
    ("apartments.new", "Neue Wohnung", "New apartment"),
    ("buildings.no_apartments", "Noch keine Wohnungen.", "No apartments yet."),
    (
        "buildings.sort_hint",
        "Die Reihenfolge in dieser Liste dient nur der Anzeige; die Kehrwoche-Rotation folgt der Anlage-Reihenfolge der Wohnungen (Badge „Rotation“).",
        "The order of this list only affects the display; the Kehrwoche rotation follows the apartments' creation order (badge “Rotation”).",
    ),
    ("apartments.drag_title", "Zum Sortieren ziehen", "Drag to sort"),
    (
        "buildings.building_owners_heading",
        "Gebäudeeigentümer",
        "Building owners",
    ),
    (
        "buildings.owners_blocked",
        "Nicht möglich, solange Wohnungen eigene Eigentümer haben (WEG).",
        "Not possible while the apartments have owners of their own (WEG).",
    ),
    ("owners.add", "Eigentümer hinzufügen", "Add owner"),
    (
        "buildings.no_building_owner",
        "Kein Eigentümer für das gesamte Gebäude erfasst – dann braucht jede Wohnung einen eigenen Eigentümer.",
        "No owner is recorded for the building as a whole – each apartment then needs an owner of its own.",
    ),
    (
        "buildings.building_owner_help",
        "Der Gebäudeeigentümer besitzt alle Wohnungen; einzelne Wohnungseigentümer sind dann nicht erforderlich. Vermietete Wohnungen delegieren die Kehrwoche weiterhin an den Mieter.",
        "The building owner owns all apartments; individual apartment owners are then not required. Rented apartments still delegate the Kehrwoche to their tenant.",
    ),
    (
        "building_owner.delete_confirm",
        "Diesen Gebäudeeigentümer löschen?",
        "Delete this building owner?",
    ),
    (
        "buildings.owner_chain_help",
        "Die Zeiträume reihen sich lückenlos aneinander: Ein neuer Eigentümer beginnt am Tag nach dem Ende des vorherigen.",
        "The periods follow each other seamlessly: a new owner starts on the day after the previous one ends.",
    ),
    ("danger_zone.heading", "Gefahrenzone", "Danger zone"),
    (
        "buildings.rotation_confirm",
        "Rotations-Versatz dieses Gebäudes ändern? Die Kehrwoche-Zuordnung aller Wochen verschiebt sich sofort – auch für bereits vergangene Wochen.",
        "Change this building's rotation offset? The Kehrwoche assignment of every week shifts immediately – including past weeks.",
    ),
    ("buildings.rotation_label", "Rotations-Versatz", "Rotation offset"),
    (
        "buildings.rotation_help",
        "Verschiebt den Beginn der Rotation: Nach jedem Schritt übernimmt die nächste Wohnung die erste Woche (0 = Standard). Gilt sofort für alle Wochen des Plans.",
        "Shifts the start of the rotation: after each step, the next apartment takes over the first week (0 = default). Applies immediately to every week of the schedule.",
    ),
    ("common.apply", "Anwenden", "Apply"),
    (
        "buildings.delete_help",
        "Löscht das Gebäude samt aller Wohnungen, Eigentümer und Mietverhältnisse.",
        "Deletes the building along with all of its apartments, owners and tenancies.",
    ),
    ("buildings.delete", "Gebäude löschen", "Delete building"),

    // --- Schedule page ---
    ("schedule.title", "Jahresplan", "Annual schedule"),
    ("schedule.resident_links", "Bewohner-Links", "Resident links"),
    (
        "schedule.back_to_building",
        "Zurück zum Gebäude",
        "Back to the building",
    ),

    // --- Apartment pages ---
    ("apartments.new_for", "Neue Wohnung: {0}", "New apartment: {0}"),
    ("apartments.edit_title", "Wohnung bearbeiten", "Edit apartment"),
    ("apartments.save", "Wohnung speichern", "Save apartment"),
    ("apartments.create", "Wohnung anlegen", "Create apartment"),
    ("apartments.first_owner", "Erster Eigentümer", "First owner"),
    (
        "apartments.building_owned_help",
        "Das Gebäude gehört als Ganzes dem Gebäudeeigentümer – die Wohnung hat daher keinen eigenen Eigentümer. Einzelne Wohnungseigentümer gibt es erst, wenn das Gebäude in Wohnungseigentum aufgeteilt wird (WEG); das unterstützt Kehrkraft nicht.",
        "The building belongs as a whole to the building owner – this apartment therefore has no owner of its own. Individual apartment owners only exist once the building is split into condominium units (WEG); Kehrkraft does not support such a split.",
    ),
    (
        "apartments.owner_required_help",
        "Eine Wohnung braucht immer mindestens einen Eigentümer; der erste wird zusammen mit der Wohnung angelegt.",
        "An apartment always needs at least one owner; the first one is created together with the apartment.",
    ),
    ("apartments.ownership_until", "Eigentum bis", "Ownership until"),
    ("apartments.owners_heading", "Eigentümer", "Owners"),
    (
        "apartments.no_owners",
        "Keine Eigentümer erfasst.",
        "No owners recorded.",
    ),
    (
        "apartments.owner_delete_confirm",
        "Dieses Eigentum löschen?",
        "Delete this ownership?",
    ),
    (
        "apartments.weg_notice",
        "Das Gebäude gehört als Ganzes dem Gebäudeeigentümer. Eigentümer einzelner Wohnungen gibt es rechtlich nicht, solange das Gebäude nicht in Wohnungseigentum aufgeteilt ist (WEG). Eine solche Aufteilung unterstützt Kehrkraft nicht.",
        "The building belongs as a whole to the building owner. There are legally no owners of individual apartments unless the building is split into condominium units (WEG). Kehrkraft does not support such a split.",
    ),
    (
        "apartments.owner_confirm_question",
        "{0} ist derzeit als Eigentümerin bzw. Eigentümer über den {1} hinaus erfasst. Soll das bisherige Eigentum am {2} (dem Tag vor dem neuen Beginn) enden?",
        "{0} is currently recorded as the owner beyond {1}. Should the previous ownership end on {2} (the day before the new one begins)?",
    ),
    (
        "apartments.owner_confirm_help",
        "Wenn Sie das nicht bestätigen, schlägt das Anlegen des neuen Eigentums fehl.",
        "If you do not confirm this, adding the new ownership will fail.",
    ),
    (
        "apartments.close_previous_owner",
        "Ja, bisheriges Eigentum beenden und hinzufügen",
        "Yes, end the previous ownership and add the new one",
    ),
    ("apartments.tenants_heading", "Mieter", "Tenants"),
    (
        "apartments.tenant_delegation_help",
        "Besteht während einer Woche ein Mietverhältnis, wird die Kehrwoche an den Mieter delegiert. Überlappende Mietverhältnisse lehnt die Anwendung ab.",
        "When a tenancy covers a week, the Kehrwoche is delegated to the tenant. Overlapping tenancies are rejected by the application.",
    ),
    (
        "apartments.no_tenancies",
        "Keine Mietverhältnisse erfasst.",
        "No tenancies recorded.",
    ),
    (
        "apartments.tenant_delete_confirm",
        "Dieses Mietverhältnis löschen?",
        "Delete this tenancy?",
    ),
    ("apartments.add_tenant", "Mieter hinzufügen", "Add tenant"),
    (
        "apartments.tenant_confirm_question",
        "Das bisherige Mietverhältnis von {0} besteht derzeit über den {1} hinaus. Soll es am {2} (dem Tag vor dem neuen Mietbeginn) enden?",
        "The previous tenancy of {0} currently runs beyond {1}. Should it end on {2} (the day before the new tenancy begins)?",
    ),
    (
        "apartments.tenant_confirm_help",
        "Wenn Sie das nicht bestätigen, schlägt das Anlegen des neuen Mietverhältnisses fehl.",
        "If you do not confirm this, adding the new tenancy will fail.",
    ),
    (
        "apartments.close_previous_tenant",
        "Ja, bisheriges Mietverhältnis beenden und hinzufügen",
        "Yes, end the previous tenancy and add the new one",
    ),
    ("tenancy.start", "Mietbeginn", "Tenancy from"),
    ("tenancy.end", "Mietende", "Tenancy until"),
    (
        "apartments.delete_confirm",
        "Diese Wohnung löschen?",
        "Delete this apartment?",
    ),

    // --- Ownership edit ---
    ("ownership.edit_title", "Eigentümer bearbeiten", "Edit owner"),
    (
        "ownership.edit_title_with",
        "Eigentümer bearbeiten: {0}",
        "Edit owner: {0}",
    ),
    ("ownership.breadcrumb", "Eigentümer: {0}", "Owner: {0}"),
    ("ownership.edit_subtitle", "Wohnung {0} ({1})", "Apartment {0} ({1})"),
    (
        "ownership.end_warning",
        "Das Enddatum {0} liegt vor dem heutigen Tag. Ab dem {1} hätte die Wohnung bis zur Erfassung eines neuen Eigentümers keinen Eigentümer mehr.",
        "The end date {0} lies before today. From {1} on, the apartment would have no owner until a new owner is recorded.",
    ),
    (
        "ownership.end_warning_help",
        "Wenn Sie das nicht beabsichtigen, wählen Sie „Nein, nicht beenden“ oder ändern Sie das Enddatum.",
        "If that is not what you intend, choose “No, don't end it” or change the end date.",
    ),
    (
        "ownership.person_help",
        "Name und E-Mail-Adresse gehören zur Person und gelten damit für alle Zeiträume, in denen sie als Eigentümerin oder Eigentümer geführt wird. Ein neues „Eigentum ab“ muss lückenlos an die übrigen Zeiträume der Wohnung anschließen.",
        "Name and e-mail address belong to the person and therefore apply to every period in which they are recorded as the owner. A new “Ownership from” date must seamlessly follow the apartment's other periods.",
    ),

    // --- Tenancy edit ---
    ("tenancy.edit_title", "Mieter bearbeiten", "Edit tenant"),
    (
        "tenancy.edit_title_with",
        "Mieter bearbeiten: {0}",
        "Edit tenant: {0}",
    ),
    ("tenancy.breadcrumb", "Mieter: {0}", "Tenant: {0}"),
    ("tenancy.edit_subtitle", "Wohnung {0} ({1})", "Apartment {0} ({1})"),
    (
        "tenancy.person_help",
        "Name und E-Mail-Adresse gehören zur Person und gelten damit für alle Zeiträume, in denen sie geführt wird – auch als Eigentümerin oder Eigentümer.",
        "Name and e-mail address belong to the person and therefore apply to every period in which they are recorded – also as an owner.",
    ),

    // --- Building-owner form ---
    (
        "building_owner.new_title",
        "Gebäudeeigentümer hinzufügen",
        "Add building owner",
    ),
    (
        "building_owner.edit_title",
        "Gebäudeeigentümer bearbeiten",
        "Edit building owner",
    ),
    (
        "building_owner.form_help",
        "Ein Gebäudeeigentümer besitzt alle Wohnungen des Gebäudes; einzelne Wohnungseigentümer sind dann nicht erforderlich.",
        "A building owner owns all apartments of the building; individual apartment owners are then not required.",
    ),
    (
        "building_owner.confirm_question",
        "{0} ist derzeit als Gebäudeeigentümer über den {1} hinaus erfasst. Soll das bisherige Gebäudeeigentum am {2} (dem Tag vor dem neuen Beginn) enden?",
        "{0} is currently recorded as the building owner beyond {1}. Should the previous building ownership end on {2} (the day before the new one begins)?",
    ),
    (
        "building_owner.confirm_help",
        "Wenn Sie das nicht bestätigen, schlägt das Anlegen des neuen Gebäudeeigentümers fehl.",
        "If you do not confirm this, adding the new building owner will fail.",
    ),
    (
        "building_owner.end_warning",
        "Das Enddatum {0} liegt vor dem heutigen Tag. Ab dem {1} hätte das Gebäude bis zur Erfassung eines neuen Gebäudeeigentümers keinen Eigentümer mehr.",
        "The end date {0} lies before today. From {1} on, the building would have no owner until a new building owner is recorded.",
    ),
    (
        "building_owner.end_warning_help",
        "Wenn Sie das nicht beabsichtigen, wählen Sie „Nein, nicht beenden“ oder ändern Sie das Enddatum.",
        "If that is not what you intend, choose “No, don't end it” or change the end date.",
    ),
    (
        "building_owner.person_help",
        "Name und E-Mail-Adresse gehören zur Person und gelten damit für alle Zeiträume, in denen sie als Eigentümerin oder Eigentümer geführt wird. Ein neues „Eigentum ab“ muss an den vorherigen Gebäudeeigentümer anschließen.",
        "Name and e-mail address belong to the person and therefore apply to every period in which they are recorded as the owner. A new “Ownership from” date must follow the previous building owner.",
    ),
    (
        "building_owner.close_previous",
        "Ja, bisheriges Gebäudeeigentum beenden und hinzufügen",
        "Yes, end the previous building ownership and add the new one",
    ),

    // --- People pages ---
    (
        "people.subtitle",
        "Jede Person wird nur einmal geführt – als Eigentümerin oder Eigentümer, Gebäudeeigentümer, Mieterin oder Mieter und Ansprechpartnerin oder Ansprechpartner. Dieselbe E-Mail-Adresse bedeutet dieselbe Person.",
        "Each person is kept only once – as an owner, building owner, tenant or contact person. The same e-mail address always means the same person.",
    ),
    (
        "people.empty",
        "Noch keine Personen erfasst. Personen entstehen zusammen mit Eigentum, Mietverhältnissen oder Ansprechpartnern.",
        "No people recorded yet. People come into being together with ownerships, tenancies or contact persons.",
    ),
    ("person.contact_heading", "Kontaktdaten", "Contact details"),
    (
        "person.email_help",
        "Die E-Mail-Adresse ist die Identität der Person: Name und Adresse gelten überall dort, wo die Person geführt wird – Eigentum, Mietverhältnisse, Ansprechpartner. Eine Adresse, die bereits einer anderen Person gehört, wird abgelehnt.",
        "The e-mail address is the person's identity: the name and address apply wherever the person is recorded – ownerships, tenancies, contacts. An address that already belongs to another person is rejected.",
    ),
    ("person.roles_heading", "Rollen", "Roles"),
    ("person.no_roles", "Keine Rollen erfasst.", "No roles recorded."),
    ("role.contact", "Ansprechpartner von", "Contact person for"),
    ("role.building_owner", "Gebäudeeigentümer von", "Building owner of"),
    ("role.owner", "Eigentümer von", "Owner of"),
    ("role.tenant", "Mieter von", "Tenant of"),
    ("role.since", "seit {0}", "since {0}"),
    ("role.range", "von {0} bis {1}", "from {0} to {1}"),
    ("role.since_in", "; seit {0}", "; since {0}"),
    ("role.range_in", "; von {0} bis {1}", "; from {0} to {1}"),

    // --- Role summaries on the people index (Rust side) ---
    (
        "role.label.contact",
        "Ansprechpartner von {0}",
        "Contact person for {0}",
    ),
    (
        "role.label.building_owner",
        "Gebäudeeigentümer von {0}",
        "Building owner of {0}",
    ),
    (
        "role.label.owner",
        "Eigentümer von {0} ({1})",
        "Owner of {0} ({1})",
    ),
    (
        "role.label.tenant",
        "Mieter von {0} ({1})",
        "Tenant of {0} ({1})",
    ),
    ("people.more_roles", " · +{0} weitere", " · +{0} more"),

    // --- Rust-side flow messages (inline errors with dates/names) ---
    (
        "ownership.decline_close",
        "Ohne das bisherige Eigentum von {0} am {1} zu beenden, kann das neue Eigentum nicht angelegt werden.",
        "Without ending the previous ownership of {0} on {1}, the new ownership cannot be created.",
    ),
    (
        "ownership.gap_dates",
        "Das Eigentum muss am Tag nach dem Ende des vorherigen Eigentums beginnen; das vorherige Eigentum endet am {0}, der neue Beginn muss am {1} liegen (nicht am {2}).",
        "The ownership must start on the day after the previous ownership ends; the previous ownership ends on {0}, so the new start must be {1} (not {2}).",
    ),
    (
        "tenancy.decline_close",
        "Ohne das bisherige Mietverhältnis von {0} am {1} zu beenden, kann das neue Mietverhältnis nicht angelegt werden.",
        "Without ending the previous tenancy of {0} on {1}, the new tenancy cannot be created.",
    ),
    (
        "building_owner.decline_close",
        "Ohne das bisherige Gebäudeeigentum von {0} am {1} zu beenden, kann der neue Gebäudeeigentümer nicht angelegt werden.",
        "Without ending the previous building ownership of {0} on {1}, the new building owner cannot be created.",
    ),
    (
        "building_owner.gap_dates",
        "Das Gebäudeeigentum muss am Tag nach dem Ende des vorherigen Gebäudeeigentums beginnen; das vorherige Gebäudeeigentum endet am {0}, der neue Beginn muss am {1} liegen (nicht am {2}).",
        "The building ownership must start on the day after the previous building ownership ends; the previous building ownership ends on {0}, so the new start must be {1} (not {2}).",
    ),

    // --- Internal error bodies (rare, shown raw) ---
    ("error.not_found", "Nicht gefunden", "Not found"),
    (
        "error.buildings_load",
        "Gebäude konnten nicht geladen werden.",
        "The buildings could not be loaded.",
    ),
    (
        "error.building_load",
        "Gebäude konnte nicht geladen werden.",
        "The building could not be loaded.",
    ),
    (
        "error.building_create",
        "Gebäude konnte nicht angelegt werden.",
        "The building could not be created.",
    ),
    (
        "error.building_save",
        "Gebäude konnte nicht gespeichert werden.",
        "The building could not be saved.",
    ),
    (
        "error.apartments_load",
        "Wohnungen konnten nicht geladen werden.",
        "The apartments could not be loaded.",
    ),
    (
        "error.apartment_load",
        "Wohnung konnte nicht geladen werden.",
        "The apartment could not be loaded.",
    ),
    (
        "error.apartment_create",
        "Wohnung konnte nicht angelegt werden.",
        "The apartment could not be created.",
    ),
    (
        "error.apartment_save",
        "Wohnung konnte nicht gespeichert werden.",
        "The apartment could not be saved.",
    ),
    (
        "error.apartment_data_load",
        "Wohnungsdaten konnten nicht geladen werden.",
        "The apartment data could not be loaded.",
    ),
    (
        "error.building_owners_load",
        "Gebäudeeigentümer konnten nicht geladen werden.",
        "The building owners could not be loaded.",
    ),
    (
        "error.building_owner_load",
        "Gebäudeeigentümer konnte nicht geladen werden.",
        "The building owner could not be loaded.",
    ),
    (
        "error.building_owner_create",
        "Gebäudeeigentümer konnte nicht angelegt werden.",
        "The building owner could not be created.",
    ),
    (
        "error.building_owner_save",
        "Gebäudeeigentümer konnte nicht gespeichert werden.",
        "The building owner could not be saved.",
    ),
    (
        "error.building_owner_delete",
        "Gebäudeeigentümer konnte nicht gelöscht werden.",
        "The building owner could not be deleted.",
    ),
    (
        "error.schedule_compute",
        "Der Plan konnte nicht berechnet werden.",
        "The schedule could not be computed.",
    ),
    (
        "error.ownership_load",
        "Eigentum konnte nicht geladen werden.",
        "The ownership could not be loaded.",
    ),
    (
        "error.ownership_create",
        "Eigentum konnte nicht angelegt werden.",
        "The ownership could not be created.",
    ),
    (
        "error.ownership_save",
        "Eigentum konnte nicht gespeichert werden.",
        "The ownership could not be saved.",
    ),
    (
        "error.ownership_delete",
        "Eigentum konnte nicht gelöscht werden.",
        "The ownership could not be deleted.",
    ),
    (
        "error.ownership_structure_load",
        "Eigentumsverhältnisse konnten nicht geladen werden.",
        "The ownership structure could not be loaded.",
    ),
    (
        "error.tenancy_load",
        "Mietverhältnis konnte nicht geladen werden.",
        "The tenancy could not be loaded.",
    ),
    (
        "error.tenancy_create",
        "Mietverhältnis konnte nicht angelegt werden.",
        "The tenancy could not be created.",
    ),
    (
        "error.tenancy_save",
        "Mietverhältnis konnte nicht gespeichert werden.",
        "The tenancy could not be saved.",
    ),
    (
        "error.people_load",
        "Personen konnten nicht geladen werden.",
        "The people could not be loaded.",
    ),
    (
        "error.person_load",
        "Person konnte nicht geladen werden.",
        "The person could not be loaded.",
    ),
    (
        "error.person_save",
        "Person konnte nicht gespeichert werden.",
        "The person could not be saved.",
    ),
    (
        "error.roles_load",
        "Rollen konnten nicht geladen werden.",
        "The roles could not be loaded.",
    ),
    (
        "error.rotation_save",
        "Rotations-Versatz konnte nicht gespeichert werden.",
        "The rotation offset could not be saved.",
    ),
    ("error.apartment_order", "Ungültige Reihenfolge der Wohnungen", "Invalid apartment order"),
    (
        "error.order_save",
        "Die Reihenfolge konnte nicht gespeichert werden.",
        "The order could not be saved.",
    ),
    (
        "error.apartments_reload",
        "Wohnungen konnten nicht neu geladen werden.",
        "The apartments could not be reloaded.",
    ),

    // --- Demo banner ---
    ("demo.banner", "Demo-Modus", "Demo mode"),

    // --- PDF (Typst poster) ---
    ("pdf.col_from", "von", "From"),
    ("pdf.col_to", "bis", "To"),
    (
        "pdf.qr_pdf",
        "Kehrwoche-PDF per QR öffnen",
        "Open the Kehrwoche PDF via QR code",
    ),
    (
        "pdf.qr_ical",
        "Kalender-Feed per QR abonnieren",
        "Subscribe to the calendar feed via QR code",
    ),
    (
        "pdf.footer_created",
        "Erstellt mit Kehrkraft v{0}",
        "Created with Kehrkraft v{0}",
    ),
    ("pdf.footer_as_of", "Stand: {0}", "As of {0}"),

    // --- iCal feed ---
    ("ical.summary", "Kehrwoche KW {0}: {1}", "Kehrwoche CW {0}: {1}"),
    ("ical.building", "Gebäude: {0}", "Building: {0}"),

    // --- Validation codes raised by the database triggers (migration 0012) ---
    ("ERR_NAME_EMPTY", "Name darf nicht leer sein.", "The name must not be empty."),
    ("ERR_NAME_TOO_LONG", "Name darf höchstens 30 Zeichen haben.", "The name must not exceed 30 characters."),
    ("ERR_EMAIL_AT", "Die E-Mail-Adresse muss ein '@' enthalten.", "The e-mail address must contain an '@'."),
    ("ERR_EMAIL_DOT", "Die E-Mail-Adresse muss nach dem '@' einen Punkt enthalten.", "The part of the e-mail address after the '@' must contain a dot."),
    ("ERR_START_DATE_FORMAT", "Startdatum muss im Format JJJJ-MM-TT vorliegen.", "The start date must use the format YYYY-MM-DD."),
    ("ERR_END_DATE_FORMAT", "Enddatum muss im Format JJJJ-MM-TT vorliegen.", "The end date must use the format YYYY-MM-DD."),
    ("ERR_START_AFTER_END", "Das Startdatum darf nicht nach dem Enddatum liegen.", "The start date must not lie after the end date."),
    ("ERR_OWNERSHIP_OVERLAP", "Das Eigentum überschneidet ein bestehendes Eigentum dieser Wohnung", "The ownership overlaps an existing ownership of this apartment"),
    ("ERR_TENANCY_OVERLAP", "Das Mietverhältnis überschneidet ein bestehendes Mietverhältnis dieser Wohnung.", "The tenancy overlaps an existing tenancy of this apartment."),
    ("ERR_BUILDING_OWNER_OVERLAP", "Das Gebäudeeigentum überschneidet ein bestehendes Gebäudeeigentum", "The building ownership overlaps an existing building ownership"),
    ("ERR_OWNERSHIP_GAP_NEXT_START", "Das Eigentum muss am Tag nach dem Ende des vorherigen Eigentums beginnen; sonst bliebe ein Zeitraum ohne Eigentümer.", "The ownership must start on the day after the previous ownership ends; otherwise there would be a period without an owner."),
    ("ERR_BUILDING_OWNER_GAP_NEXT_START", "Das Gebäudeeigentum muss am Tag nach dem Ende des vorherigen Gebäudeeigentums beginnen; sonst bliebe ein Zeitraum ohne Eigentümer.", "The building ownership must start on the day after the previous building ownership ends; otherwise there would be a period without an owner."),
    ("ERR_OWNERSHIP_GAP_PREV_END", "Das Eigentum muss am Tag vor dem Beginn des nächsten Eigentums enden; sonst bliebe ein Zeitraum ohne Eigentümer.", "The ownership must end on the day before the next ownership begins; otherwise there would be a period without an owner."),
    ("ERR_BUILDING_OWNER_GAP_PREV_END", "Das Gebäudeeigentum muss am Tag vor dem Beginn des nächsten Gebäudeeigentums enden; sonst bliebe ein Zeitraum ohne Eigentümer.", "The building ownership must end on the day before the next building ownership begins; otherwise there would be a period without an owner."),
    ("ERR_OWNERSHIP_LAST_DELETE", "Das letzte Eigentum einer Wohnung kann nicht gelöscht werden, sonst hat die Wohnung keinen Eigentümer.", "The last ownership of an apartment cannot be deleted, or the apartment would have no owner."),
    ("ERR_OWNERSHIP_MIDDLE_DELETE", "Dieses Eigentum liegt zwischen zwei anderen Eigentümerzeiträumen. Es kann nur das erste oder das letzte Eigentum gelöscht werden.", "This ownership lies between two other ownership periods. Only the first or the last ownership can be deleted."),
    ("ERR_BUILDING_OWNER_LAST_DELETE", "Der letzte Gebäudeeigentümer kann nicht gelöscht werden, solange Wohnungen ohne eigenen Eigentümer auf ihn angewiesen sind.", "The last building owner cannot be deleted while apartments without an owner of their own depend on them."),
    ("ERR_BUILDING_OWNER_MIDDLE_DELETE", "Dieses Gebäudeeigentum liegt zwischen zwei anderen Eigentümerzeiträumen. Es kann nur das erste oder das letzte Eigentum gelöscht werden.", "This building ownership lies between two other ownership periods. Only the first or the last ownership can be deleted."),
    ("ERR_APARTMENT_REQUIRES_OWNER", "Eine Wohnung muss mindestens ein Eigentum haben. Legen Sie die Wohnung zusammen mit ihrem ersten Eigentümer an, oder legen Sie zuerst einen Eigentümer für das gesamte Gebäude an.", "An apartment must have at least one ownership. Create the apartment together with its first owner, or first record an owner for the whole building."),
    ("ERR_OWNERSHIP_REQUIRES_PERSON", "Ein Eigentum muss einer Person zugeordnet sein.", "An ownership must belong to a person."),
    ("ERR_BUILDING_OWNER_REQUIRES_PERSON", "Ein Gebäudeeigentum muss einer Person zugeordnet sein.", "A building ownership must belong to a person."),
    ("ERR_TENANCY_REQUIRES_PERSON", "Ein Mietverhältnis muss einer Person zugeordnet sein.", "A tenancy must belong to a person."),
    ("ERR_ADMIN_REQUIRES_PERSON", "Ein Ansprechpartner muss einer Person zugeordnet sein.", "A contact person must belong to a person."),
    ("ERR_PERSON_EMAIL_TAKEN", "Eine Person mit dieser E-Mail-Adresse existiert bereits.", "A person with this e-mail address already exists."),
    ("ERR_OWNERSHIP_FORBIDDEN_WHEN_BUILDING_OWNER", "Eine Wohnung eines Gebäudes mit Gebäudeeigentümer kann keinen eigenen Eigentümer haben.", "An apartment of a building with a building owner cannot have an owner of its own."),
    ("ERR_BUILDING_OWNER_FORBIDDEN_WHEN_APARTMENT_OWNERS", "Ein Gebäude, dessen Wohnungen eigene Eigentümer haben, kann keinen Gebäudeeigentümer haben.", "A building whose apartments have owners of their own cannot have a building owner."),
    ("ERR_OWNERSHIP_CURRENT_DELETE", "Das Eigentum, das die Wohnung derzeit abdeckt, kann nicht gelöscht werden. Beenden Sie den Zeitraum stattdessen oder legen Sie zuerst einen neuen Eigentümer an.", "The ownership currently covering the apartment cannot be deleted. End the period instead, or first record a new owner."),
    ("ERR_BUILDING_OWNER_CURRENT_DELETE", "Der Gebäudeeigentümer, der das Gebäude derzeit abdeckt, kann nicht gelöscht werden, solange Wohnungen auf ihn angewiesen sind. Beenden Sie den Zeitraum stattdessen oder legen Sie zuerst einen Nachfolger an.", "The building owner currently covering the building cannot be deleted while apartments depend on them. End the period instead, or first record a successor."),
];

/// Look the key up in the catalog.
fn entry(key: &str) -> Option<&'static Entry> {
    MESSAGES.iter().find(|(k, _, _)| *k == key)
}

/// The message for `key` in `lang`. An unknown key renders as the key
/// itself (visible immediately in the UI, so a typo cannot hide); the
/// catalog test below keeps the table free of duplicates.
pub fn msg(lang: Lang, key: &'static str) -> &'static str {
    let Some((_, de, en)) = entry(key) else {
        tracing::error!(key, "missing translation key");
        return key;
    };
    match lang {
        Lang::De => de,
        Lang::En => en,
    }
}

/// [`msg`] with `{0}`/`{1}`/… placeholders substituted by `args`.
pub fn msgf(lang: Lang, key: &'static str, args: &[&str]) -> String {
    substitute(msg(lang, key), args)
}

/// Replace `{0}`..`{9}` placeholders with `args[i]`. Unknown markers and
/// markers beyond `args` stay untouched so mistakes remain visible.
fn substitute(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        if let Ok(idx) = after[..end].parse::<usize>() {
            if let Some(arg) = args.get(idx) {
                out.push_str(arg);
                rest = &after[end + 1..];
                continue;
            }
        }
        // Not a valid placeholder: keep the braces as-is.
        out.push('{');
        rest = after;
    }
    let _ = out.write_str(rest);
    out
}

/// Translate a message that came out of the database. Since migration 0012
/// the validation triggers raise `ERR_*` codes (catalog keys); older or
/// unexpected messages are passed through unchanged.
pub fn db_msg(lang: Lang, message: &str) -> String {
    match entry(message) {
        Some((_, de, en)) => match lang {
            Lang::De => de.to_string(),
            Lang::En => en.to_string(),
        },
        None => message.to_string(),
    }
}

/// The strftime pattern used to render calendar dates in the UI: German
/// `21.09.2026`, English (unambiguous) `2026-09-21`.
pub fn date_format(lang: Lang) -> &'static str {
    match lang {
        Lang::De => "%d.%m.%Y",
        Lang::En => "%Y-%m-%d",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_no_duplicate_keys() {
        let mut keys: Vec<&str> = MESSAGES.iter().map(|(k, _, _)| *k).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "duplicate catalog key");
    }

    #[test]
    fn catalog_placeholders_match_between_languages() {
        for (key, de, en) in MESSAGES {
            assert_eq!(
                placeholders(de),
                placeholders(en),
                "placeholder mismatch for {key:?}"
            );
        }
    }

    fn placeholders(s: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut rest = s;
        while let Some(start) = rest.find('{') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('}') else { break };
            if let Ok(idx) = after[..end].parse::<usize>() {
                out.push(idx);
            }
            rest = &after[end + 1..];
        }
        out
    }

    #[test]
    fn cookie_wins_over_accept_language() {
        assert_eq!(
            negotiate(Some("en"), Some("de-DE,de;q=0.9")),
            Lang::En,
            "an explicit cookie choice always wins"
        );
        assert_eq!(negotiate(Some("de"), Some("en-US,en;q=0.9")), Lang::De);
    }

    #[test]
    fn accept_language_picks_first_supported() {
        assert_eq!(negotiate(None, Some("en-US,en;q=0.9,de;q=0.8")), Lang::En);
        assert_eq!(negotiate(None, Some("de-DE,de;q=0.9")), Lang::De);
        assert_eq!(negotiate(None, Some("fr,fr-CH;q=0.8,de;q=0.5")), Lang::De);
    }

    #[test]
    fn accept_language_q_weights_are_honoured() {
        assert_eq!(negotiate(None, Some("de;q=0.8, en;q=0.9")), Lang::En);
        assert_eq!(negotiate(None, Some("de;q=0, en;q=0")), Lang::De);
    }

    #[test]
    fn missing_preference_falls_back_to_german() {
        assert_eq!(negotiate(None, None), Lang::De);
        assert_eq!(negotiate(None, Some("fr")), Lang::De);
        assert_eq!(negotiate(Some("fr"), None), Lang::De);
        assert_eq!(negotiate(Some(""), None), Lang::De);
    }

    #[test]
    fn cookie_values_are_parsed_by_name() {
        assert_eq!(
            cookie_value("session=abc; lang=en; theme=dark", COOKIE_NAME),
            Some("en")
        );
        assert_eq!(cookie_value("lang=de", COOKIE_NAME), Some("de"));
        assert_eq!(cookie_value("langauge=de", COOKIE_NAME), None);
    }

    #[test]
    fn placeholders_substitute_in_order() {
        assert_eq!(substitute("a {0} b {1} c", &["x", "y"]), "a x b y c");
        assert_eq!(substitute("only {1}", &["x", "y"]), "only y");
        assert_eq!(substitute("{0}{0}", &["x"]), "xx");
        // Missing arguments and unknown markers stay visible.
        assert_eq!(substitute("a {0} {1}", &["x"]), "a x {1}");
        assert_eq!(substitute("a {x} b", &["y"]), "a {x} b");
    }

    #[test]
    fn db_codes_map_to_messages() {
        assert_eq!(
            db_msg(Lang::De, "ERR_NAME_EMPTY"),
            "Name darf nicht leer sein."
        );
        assert_eq!(
            db_msg(Lang::En, "ERR_NAME_EMPTY"),
            "The name must not be empty."
        );
        // Unknown messages pass through untouched (pre-0012 databases).
        assert_eq!(
            db_msg(Lang::En, "Name darf nicht leer sein."),
            "Name darf nicht leer sein."
        );
    }

    #[test]
    fn ui_helper_methods_resolve_messages() {
        let de = Ui::for_lang(Lang::De);
        let en = Ui::for_lang(Lang::En);
        assert_eq!(de.t("nav.buildings"), "Gebäude");
        assert_eq!(en.t("nav.buildings"), "Buildings");
        assert_eq!(de.lang_code(), "de");
        assert_eq!(en.lang_code(), "en");
        assert_eq!(en.t1("role.since", "2026-01-01"), "since 2026-01-01");
    }
}
