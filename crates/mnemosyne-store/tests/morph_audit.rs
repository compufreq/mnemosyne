//! TEMPORARY morphological-coverage audit — NOT part of the suite.
//!
//! One instrument, applied per language:
//!   1. end-to-end admission through the real `search`, at TWO drawer lengths
//!      (the short one is where the cosine masks the defect);
//!   2. a per-mechanism breakdown, so "what works" is a statement about
//!      morphology rather than an average;
//!   3. a counterfactual tier analysis over whatever was DROPPED — which
//!      string relation, if any, would have reached it.
//!
//! Admission is measured with the real engine. The tier analysis is pure
//! string math over `search_key` output, which is the real fold.
//!
//! Run: cargo test -p mnemosyne-store --test morph_audit -- --nocapture

use mnemosyne_core::normalize::search_key;
use mnemosyne_core::Drawer;
use mnemosyne_store::{PalaceStore, SearchOptions};
use mnemosyne_vault::{SecurityLevel, VaultManager};
use tempfile::TempDir;

fn drawer(content: &str, idx: u32) -> Drawer {
    Drawer::new("w", "r", content.into(), Some("t.md".into()), idx, "test")
}

fn store() -> (TempDir, PalaceStore) {
    let dir = TempDir::new().unwrap();
    let mgr = VaultManager::open(dir.path(), None).unwrap();
    let vault = mgr.create("test", SecurityLevel::Sealed).unwrap();
    (dir, PalaceStore::open(vault).unwrap())
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ch {
    Exact,
    Morph,
    SemOnly,
    Dropped,
}

impl Ch {
    fn label(self) -> &'static str {
        match self {
            Ch::Exact => "EXACT",
            Ch::Morph => "morph",
            Ch::SemOnly => "sem-only",
            Ch::Dropped => "DROPPED",
        }
    }
    fn admitted(self) -> bool {
        self != Ch::Dropped
    }
}

/// (query lemma, form appearing in the drawer, mechanism tag)
type Pair = (&'static str, &'static str, &'static str);

struct Lang {
    name: &'static str,
    /// Sentence frame: `{}` is where the inflected form goes.
    frame: &'static str,
    filler: &'static [&'static str],
    pairs: &'static [Pair],
}

fn pad(core: &str, filler: &[&str]) -> String {
    let mut s = String::from(core);
    for f in filler {
        s.push(' ');
        s.push_str(f);
    }
    s
}

/// The padding must not contain any query lemma, or the long-drawer condition
/// measures the padding instead of the morphology: a target drawer padded with
/// text containing the query is admitted on EXACT no matter what the pair
/// does. This caught a real contamination in the Arabic filler (`كبير`,
/// `صغير`, `بيت`, `المدينة`, `مصرف`), which made broken-plural admission
/// appear to RISE with drawer length.
fn assert_padding_is_disjoint(l: &Lang) {
    let pad_text = format!("{} {}", l.frame.replace("{}", " "), l.filler.join(" "));
    let folded = search_key(&pad_text).to_string();
    for (q, _, _) in l.pairs {
        let qk = search_key(q);
        assert!(
            !folded.contains(&*qk),
            "{}: padding contains the query lemma {:?} — the long-drawer \
             condition would measure the padding, not the pair",
            l.name,
            q
        );
    }
}

fn probe(l: &Lang, query: &str, form: &str, long: bool) -> Ch {
    let (_d, mut s) = store();
    let core = l.frame.replace("{}", form);
    let target = if long { pad(&core, l.filler) } else { core.clone() };
    s.upsert(&drawer(&target, 0)).unwrap();
    for (i, f) in l.filler.iter().enumerate() {
        let fill = if long { pad(f, l.filler) } else { f.to_string() };
        s.upsert(&drawer(&fill, i as u32 + 1)).unwrap();
    }
    let hits = s.search(query, &SearchOptions::default()).unwrap();
    match hits.iter().find(|h| h.drawer.content.contains(form)) {
        None => Ch::Dropped,
        Some(h) => {
            if h.lexical_exact > 0.0 {
                Ch::Exact
            } else if h.lexical_morph > 0.0 {
                Ch::Morph
            } else {
                Ch::SemOnly
            }
        }
    }
}

// --- counterfactual string relations, over folded forms ---------------------

fn contains_at(q: &str, t: &str, floor: usize) -> bool {
    let (qn, tn) = (q.chars().count(), t.chars().count());
    if qn.min(tn) < floor {
        return false;
    }
    if qn <= tn {
        t.contains(q)
    } else {
        q.contains(t)
    }
}

fn shared_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// Every character of `q`, in order, somewhere in `t` — the relation Arabic
/// root-and-pattern morphology actually preserves.
fn is_subsequence(q: &str, t: &str) -> bool {
    let mut it = t.chars();
    q.chars().all(|c| it.any(|x| x == c))
}

/// Drop the Arabic weak letters (alef, waw, yeh). What remains is close to the
/// consonantal skeleton, which is what the templatic patterns hold constant
/// while they rearrange everything around it. This is the classical "light
/// root" move — and the reason it is dangerous is the same reason it works:
/// it collapses aggressively, so its false-merge rate is the whole question.
fn skeleton(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c as u32, 0x0627 | 0x0648 | 0x064A))
        .collect()
}

#[derive(Default)]
struct Summary {
    name: String,
    total: usize,
    short_ok: usize,
    long_ok: usize,
    dropped: usize,
    floor3: usize,
    prefix7: usize,
    subseq: usize,
    skel_sub: usize,
    nothing: usize,
}

fn run(l: &Lang) -> Summary {
    println!("\n\n########## {} ##########", l.name);
    assert_padding_is_disjoint(l);
    let mut sum = Summary {
        name: l.name.to_string(),
        total: l.pairs.len(),
        ..Default::default()
    };
    let mut long_res: Vec<(usize, Ch)> = Vec::new();

    for &long in &[false, true] {
        let mut admitted = 0usize;
        println!(
            "\n--- {} · drawer = {} ---",
            l.name,
            if long { "~70 words" } else { "1 sentence" }
        );
        for (i, (q, f, mech)) in l.pairs.iter().enumerate() {
            let ch = probe(l, q, f, long);
            if ch.admitted() {
                admitted += 1;
            }
            if long {
                long_res.push((i, ch));
            }
            println!("  {:<14} {:<16} {:<22} {}", q, f, mech, ch.label());
        }
        println!(
            "  => ADMITTED {}/{} = {:.1}%",
            admitted,
            l.pairs.len(),
            100.0 * admitted as f32 / l.pairs.len() as f32
        );
        if long {
            sum.long_ok = admitted;
        } else {
            sum.short_ok = admitted;
        }
    }

    // Per-mechanism breakdown at realistic length.
    println!("\n--- {} · by mechanism (realistic length) ---", l.name);
    let mut mechs: Vec<&str> = l.pairs.iter().map(|p| p.2).collect();
    mechs.sort_unstable();
    mechs.dedup();
    for m in mechs {
        let idxs: Vec<usize> = l
            .pairs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.2 == m)
            .map(|(i, _)| i)
            .collect();
        let ok = idxs
            .iter()
            .filter(|i| long_res.iter().any(|(j, c)| j == *i && c.admitted()))
            .count();
        println!(
            "  {:<24} {:>2}/{:<2}  {:>5.1}%",
            m,
            ok,
            idxs.len(),
            100.0 * ok as f32 / idxs.len() as f32
        );
    }

    // Tier analysis over what was dropped.
    let dropped: Vec<usize> = long_res
        .iter()
        .filter(|(_, c)| !c.admitted())
        .map(|(i, _)| *i)
        .collect();
    println!(
        "\n--- {} · what would recover the {} dropped ---",
        l.name,
        dropped.len()
    );
    sum.dropped = dropped.len();
    if dropped.is_empty() {
        return sum;
    }
    let mut by_floor = [0usize; 9];
    let (mut swf, mut subseq, mut skel, mut skel_sub) = (0usize, 0usize, 0usize, 0usize);
    let mut nothing: Vec<String> = Vec::new();
    for &i in &dropped {
        let (q, f, _) = l.pairs[i];
        let (qk, fk) = (search_key(q), search_key(f));
        for (floor, slot) in by_floor.iter_mut().enumerate().skip(1) {
            if contains_at(&qk, &fk, floor) {
                *slot += 1;
            }
        }
        let p = shared_prefix(&qk, &fk);
        let is_swf = p >= 7 && qk.chars().count().min(fk.chars().count()) - p <= 3;
        let is_sub = is_subsequence(&qk, &fk) || is_subsequence(&fk, &qk);
        let (sq, sf) = (skeleton(&qk), skeleton(&fk));
        let is_skel = !sq.is_empty() && sq == sf;
        let is_skel_sub =
            !sq.is_empty() && (is_subsequence(&sq, &sf) || is_subsequence(&sf, &sq));
        if is_swf {
            swf += 1;
        }
        if is_sub {
            subseq += 1;
        }
        if is_skel {
            skel += 1;
        }
        if is_skel_sub {
            skel_sub += 1;
        }
        if !is_swf && !is_sub && !is_skel_sub && !contains_at(&qk, &fk, 3) {
            nothing.push(format!("{q} -> {f} (prefix {p}, skel {sq} vs {sf})"));
        }
    }
    let n = dropped.len();
    for floor in 3..=8 {
        println!("  containment floor {floor:<2}      recovers {:>2}/{n}", by_floor[floor]);
    }
    println!("  shared-prefix >=7 rule    recovers {swf:>2}/{n}");
    println!("  ordered SUBSEQUENCE       recovers {subseq:>2}/{n}");
    println!("  consonant skeleton EQUAL  recovers {skel:>2}/{n}");
    println!("  skeleton SUBSEQUENCE      recovers {skel_sub:>2}/{n}");
    println!("\n  Reached by NO string relation: {}/{n}", nothing.len());
    for x in &nothing {
        println!("    - {x}");
    }
    sum.floor3 = by_floor[3];
    sum.prefix7 = swf;
    sum.subseq = subseq;
    sum.skel_sub = skel_sub;
    sum.nothing = nothing.len();
    sum
}

// =============================================================== ARABIC =====

// Deliberately avoids every query lemma — see `assert_padding_is_disjoint`.
// `المدينة`, `الصغير`, `صغيرا`, `مصرف` and `البيت` were all removed: each
// either is a query lemma or contains one.
const AR_FILLER: &[&str] = &[
    "الطقس جميل اليوم في الحي القديم",
    "ذهبت إلى المستشفى أمس مع أخي",
    "اشتريت قطارا لابني من السوق",
    "بنك ضخم في وسط البلد يفتح مبكرا",
    "تناولنا العشاء في مطعم قريب من المنزل",
    "سافرت بالطائرة إلى بلد بعيد الشهر الماضي",
];

const AR_PAIRS: &[Pair] = &[
    // --- concatenative: clitics attach at the edges, stem stays contiguous
    ("كتاب", "الكتاب", "concat: article"),
    ("كتاب", "بالكتاب", "concat: prep+article"),
    ("كتاب", "والكتاب", "concat: conj+article"),
    ("كتاب", "للكتاب", "concat: lam+article"),
    ("كتاب", "كتابه", "concat: enclitic his"),
    ("كتاب", "كتابها", "concat: enclitic her"),
    ("كتاب", "كتابهم", "concat: enclitic their"),
    ("كتاب", "كتابان", "concat: dual"),
    ("كتاب", "كتابين", "concat: dual oblique"),
    ("معلم", "المعلم", "concat: article"),
    ("معلم", "معلمون", "concat: sound masc pl"),
    ("معلم", "معلمين", "concat: sound masc pl obl"),
    ("معلم", "معلمة", "concat: feminine"),
    ("معلم", "معلمات", "concat: sound fem pl"),
    ("مدرسة", "المدرسة", "concat: article"),
    ("طالب", "الطالب", "concat: article"),
    ("طالب", "طالبة", "concat: feminine"),
    ("مصر", "مصري", "concat: nisba"),
    ("كتب", "يكتب", "concat: verb prefix"),
    ("كتب", "تكتب", "concat: verb prefix"),
    ("كتب", "يكتبون", "concat: verb circumfix"),
    ("كتب", "كتبت", "concat: verb suffix"),
    ("كتب", "كتبوا", "concat: verb suffix"),
    ("كتب", "مكتب", "concat: place noun"),
    ("كتب", "مكتبة", "concat: place noun fem"),
    // --- templatic: the root is a SUBSEQUENCE, not a substring
    ("كتاب", "كتب", "templatic: broken pl"),
    ("رجل", "رجال", "templatic: broken pl"),
    ("طالب", "طلاب", "templatic: broken pl"),
    ("عامل", "عمال", "templatic: broken pl"),
    ("راكب", "ركاب", "templatic: broken pl"),
    ("بيت", "بيوت", "templatic: broken pl"),
    ("مدينة", "مدن", "templatic: broken pl"),
    ("قلم", "أقلام", "templatic: broken pl"),
    ("ولد", "أولاد", "templatic: broken pl"),
    ("كبير", "كبار", "templatic: broken pl"),
    ("صغير", "صغار", "templatic: broken pl"),
    ("كتب", "كاتب", "templatic: act participle"),
    ("كتب", "مكتوب", "templatic: pass participle"),
    ("كتب", "كتابة", "templatic: masdar"),
    ("كاتب", "كتاب", "templatic: pattern shift"),
    ("كبير", "أكبر", "templatic: elative"),
    ("امرأة", "نساء", "suppletive"),
];

// Persian rides along: SAME SCRIPT, different typology. If script were the
// right axis, these would behave like Arabic. They do not.
const FA_PAIRS: &[Pair] = &[
    ("کتاب", "کتابها", "fa concat: plural"),
    ("کتاب", "کتاب‌ها", "fa concat: plural ZWNJ"),
    ("خانه", "خانه‌ها", "fa concat: plural ZWNJ"),
    ("دانشجو", "دانشجویان", "fa concat: plural -an"),
    ("کتاب", "کتابم", "fa concat: possessive"),
    ("رفتن", "می‌روم", "fa: suppletive verb stem"),
];

// ================================================================ GREEK =====

const EL_FILLER: &[&str] = &[
    "Ο καιρός ήταν πολύ ζεστός χθες το απόγευμα στην παραλία",
    "Αγόρασα φρέσκα λαχανικά από τη λαϊκή αγορά της γειτονιάς",
    "Το τρένο καθυστέρησε δύο ώρες εξαιτίας της απεργίας",
    "Μαγείρεψα φασολάδα και έφτιαξα μια σαλάτα με ντομάτα",
    "Η ταινία που είδαμε το Σάββατο δεν μου άρεσε καθόλου",
    "Πλήρωσα τον λογαριασμό του ρεύματος μέσω της τράπεζας",
];

const EL_PAIRS: &[Pair] = &[
    ("άνθρωπος", "ανθρώπου", "noun: gen sg (subst)"),
    ("άνθρωπος", "άνθρωπο", "noun: acc sg (trunc)"),
    ("άνθρωπος", "άνθρωποι", "noun: nom pl (subst)"),
    ("άνθρωπος", "ανθρώπους", "noun: acc pl (subst)"),
    ("άνθρωπος", "ανθρώπων", "noun: gen pl (subst)"),
    ("δρόμος", "δρόμου", "noun: gen sg (subst)"),
    ("δρόμος", "δρόμοι", "noun: nom pl (subst)"),
    ("εργαζόμενος", "εργαζομένου", "noun: gen sg (subst)"),
    ("εργαζόμενος", "εργαζόμενο", "noun: acc sg (trunc)"),
    ("εργαζόμενος", "εργαζόμενοι", "noun: nom pl (subst)"),
    ("θάλασσα", "θάλασσας", "noun: gen sg (additive)"),
    ("θάλασσα", "θάλασσες", "noun: nom pl (subst)"),
    ("θάλασσα", "θαλασσών", "noun: gen pl (subst)"),
    ("ημέρα", "ημέρας", "noun: gen sg (additive)"),
    ("ημέρα", "ημέρες", "noun: nom pl (subst)"),
    ("πόλη", "πόλης", "noun: gen sg (additive)"),
    ("πόλη", "πόλεις", "noun: nom pl (subst)"),
    ("κυβέρνηση", "κυβέρνησης", "noun: gen sg (additive)"),
    ("κυβέρνηση", "κυβερνήσεις", "noun: nom pl (subst)"),
    ("πληροφορία", "πληροφορίας", "noun: gen sg (additive)"),
    ("πληροφορία", "πληροφορίες", "noun: nom pl (subst)"),
    ("εφημερίδα", "εφημερίδας", "noun: gen sg (additive)"),
    ("εφημερίδα", "εφημερίδες", "noun: nom pl (subst)"),
    ("άντρας", "άντρα", "noun: gen sg (trunc)"),
    ("άντρας", "άντρες", "noun: nom pl (subst)"),
    ("μαθητής", "μαθητή", "noun: gen sg (trunc)"),
    ("μαθητής", "μαθητές", "noun: nom pl (subst)"),
    ("μαθητής", "μαθητών", "noun: gen pl (subst)"),
    ("βιβλίο", "βιβλίου", "noun: gen sg (additive)"),
    ("βιβλίο", "βιβλία", "noun: nom pl (subst)"),
    ("παιδί", "παιδιού", "noun: gen sg (additive)"),
    ("παιδί", "παιδιά", "noun: nom pl (subst)"),
    ("πρόβλημα", "προβλήματος", "noun: gen sg (additive)"),
    ("πρόβλημα", "προβλήματα", "noun: nom pl (additive)"),
    ("πρόβλημα", "προβλημάτων", "noun: gen pl (additive)"),
    ("όνομα", "ονόματος", "noun: gen sg (additive)"),
    ("όνομα", "ονόματα", "noun: nom pl (additive)"),
    ("πρόγραμμα", "προγράμματος", "noun: gen sg (additive)"),
    ("πρόγραμμα", "προγράμματα", "noun: nom pl (additive)"),
    ("γράφω", "γράφει", "verb: 3sg pres"),
    ("γράφω", "γράφουμε", "verb: 1pl pres"),
    ("γράφω", "έγραψα", "verb: aorist (augment)"),
    ("διαβάζω", "διαβάζει", "verb: 3sg pres"),
    ("διαβάζω", "διάβασα", "verb: aorist (augment)"),
    ("πηγαίνω", "πηγαίνει", "verb: 3sg pres"),
    ("πηγαίνω", "πήγα", "verb: aorist (augment)"),
    ("βλέπω", "βλέπει", "verb: 3sg pres"),
    ("βλέπω", "είδα", "verb: suppletive"),
    ("εργάζομαι", "εργάζεται", "verb: 3sg pres"),
];





// ======================================================= ROUND 2: OTHERS ====
//
// Paradigm confidence is stated per language. A wrong paradigm produces a
// wrong percentage, and that is my error rather than a finding, so the low-
// confidence rows say so instead of quietly inflating the table.

const RU_FILLER: &[&str] = &[
    "Погода вчера была тёплая и солнечная",
    "Я купил свежие овощи на рынке утром",
    "Поезд опоздал на два часа из-за ремонта",
    "Мы ужинали в ресторане недалеко отсюда",
];
/// HIGH confidence. Six cases, plus a consonant-mutating verb and suppletion.
const RU_PAIRS: &[Pair] = &[
    ("книга", "книги", "noun: gen sg (subst)"),
    ("книга", "книге", "noun: dat sg (subst)"),
    ("книга", "книгу", "noun: acc sg (subst)"),
    ("книга", "книгой", "noun: inst sg (subst)"),
    ("город", "города", "noun: gen sg (additive)"),
    ("город", "городом", "noun: inst sg (additive)"),
    ("город", "городах", "noun: prep pl (additive)"),
    ("человек", "человека", "noun: gen sg (additive)"),
    ("человек", "люди", "noun: suppletive pl"),
    ("писать", "пишет", "verb: stem mutation с/ш"),
    ("писать", "писал", "verb: past"),
    ("работать", "работает", "verb: 3sg pres"),
];

const DE_FILLER: &[&str] = &[
    "Das Wetter war gestern warm und sonnig",
    "Ich kaufte frisches Gemüse auf dem Markt",
    "Der Zug hatte zwei Stunden Verspätung",
    "Wir aßen in einem Restaurant in der Nähe",
];
/// HIGH confidence. The umlaut plural is the interesting case: it mutates
/// INSIDE the stem, but the fold strips the umlaut, so containment survives.
const DE_PAIRS: &[Pair] = &[
    ("Buch", "Bücher", "noun: umlaut pl"),
    ("Haus", "Häuser", "noun: umlaut pl"),
    ("Kind", "Kinder", "noun: additive pl"),
    ("Konfiguration", "Konfigurationen", "noun: additive pl (long)"),
    ("Dampfschiff", "Donaudampfschifffahrt", "compound: interior"),
    ("Ausbildung", "Bundesausbildungsgesetz", "compound: interior"),
    ("gehen", "ging", "verb: ablaut"),
    ("sprechen", "spricht", "verb: stem vowel change"),
];

const ES_FILLER: &[&str] = &[
    "El tiempo ayer estuvo cálido y soleado",
    "Compré verduras frescas en el mercado",
    "El tren llegó con dos horas de retraso",
    "Cenamos en un restaurante cercano",
];
/// HIGH confidence.
const ES_PAIRS: &[Pair] = &[
    ("libro", "libros", "noun: additive pl"),
    ("ciudad", "ciudades", "noun: additive pl"),
    ("información", "informaciones", "noun: additive pl (long)"),
    ("hablar", "hablo", "verb: 1sg pres"),
    ("hablar", "habló", "verb: 3sg pret"),
    ("hablar", "hablaron", "verb: 3pl pret"),
    ("ser", "fue", "verb: suppletive"),
];

const TR_FILLER: &[&str] = &[
    "Hava dün sıcak ve güneşliydi",
    "Pazardan taze sebze aldım",
    "Tren iki saat gecikmeyle geldi",
    "Yakındaki bir lokantada yemek yedik",
];
/// HIGH confidence. Turkish is purely additive — vowel harmony changes the
/// SUFFIX, never the stem — so every pair is containment-true and the only
/// thing that can block it is the length floor.
const TR_PAIRS: &[Pair] = &[
    ("ev", "evler", "agglut: plural"),
    ("ev", "evlerde", "agglut: plural+locative"),
    ("kitap", "kitaplar", "agglut: plural"),
    ("kitap", "kitaplarımızdan", "agglut: 4 suffixes"),
    ("bilgisayar", "bilgisayarlar", "agglut: plural (long)"),
    ("gelmek", "geliyorum", "verb: progressive"),
];

const HE_FILLER: &[&str] = &[
    "מזג האוויר אתמול היה חם ושמשי",
    "קניתי ירקות טריים בשוק",
    "הרכבת איחרה בשעתיים",
    "אכלנו במסעדה קרובה",
];
/// HIGH confidence on the forms. Hebrew is the structural surprise: it uses
/// spaces, so `Script::Other` classes it as DELIMITING — but its clitics
/// attach with no delimiter, exactly like Arabic's. If that classification is
/// what blocks it, the fix is one line rather than a lexicon.
const HE_PAIRS: &[Pair] = &[
    ("ספר", "הספר", "clitic: definite (front)"),
    ("ספר", "בספר", "clitic: preposition (front)"),
    ("ספר", "ספרים", "noun: plural"),
    ("ספר", "הספרים", "clitic + plural"),
    ("ילד", "ילדים", "noun: plural"),
    ("ילד", "הילדים", "clitic + plural"),
    ("כתב", "מכתב", "templatic: contiguity kept"),
    ("כתב", "כותב", "templatic: infix breaks root"),
];

const KO_FILLER: &[&str] = &[
    "어제 날씨는 따뜻하고 맑았다",
    "시장에서 신선한 채소를 샀다",
    "기차가 두 시간 늦게 도착했다",
    "근처 식당에서 저녁을 먹었다",
];
/// MEDIUM confidence on paradigm choice, high on the forms themselves.
const KO_PAIRS: &[Pair] = &[
    ("한국어", "한국어는", "agglut: topic particle"),
    ("한국어", "한국어를", "agglut: object particle"),
    ("학교", "학교에서", "agglut: locative"),
    ("하다", "해요", "verb: contraction, no shared char"),
    ("먹다", "먹었어요", "verb: past polite"),
];

const JA_FILLER: &[&str] = &[
    "昨日の天気は暖かくて晴れていた",
    "市場で新鮮な野菜を買った",
    "電車が二時間遅れて到着した",
    "近くのレストランで夕食を食べた",
];
/// HIGH confidence.
const JA_PAIRS: &[Pair] = &[
    ("東京", "東京で", "particle attachment"),
    ("会議", "会議室", "compound"),
    ("書く", "書いた", "verb: past (kana changes)"),
    ("書く", "書きます", "verb: polite"),
    ("食べる", "食べた", "verb: past"),
];

const ZH_FILLER: &[&str] = &[
    "昨天的天气温暖晴朗",
    "我在市场买了新鲜的蔬菜",
    "火车晚点了两个小时",
    "我们在附近的餐馆吃了晚饭",
];
/// CONTROL — Chinese is isolating, so there is no morphology to miss. If this
/// row is not near 100% the harness is wrong, not the engine.
const ZH_PAIRS: &[Pair] = &[
    ("北京", "北京市", "compound"),
    ("学习", "学习了", "aspect particle"),
    ("电脑", "电脑上", "locative"),
    ("会议", "会议室", "compound"),
];

const TH_FILLER: &[&str] = &[
    "เมื่อวานอากาศอบอุ่นและแดดจ้า",
    "ฉันซื้อผักสดจากตลาด",
    "รถไฟมาช้าไปสองชั่วโมง",
    "เรากินข้าวเย็นที่ร้านใกล้บ้าน",
];
/// CONTROL — Thai is isolating but unsegmented, so this tests the bigram path
/// independently of morphology.
const TH_PAIRS: &[Pair] = &[
    ("หนังสือ", "หนังสือเล่มนี้", "classifier phrase"),
    ("โรงเรียน", "โรงเรียนของเรา", "possessive phrase"),
    ("ประชุม", "การประชุม", "nominaliser prefix"),
];

const HI_FILLER: &[&str] = &[
    "कल मौसम गर्म और धूप वाला था",
    "मैंने बाजार से ताजी सब्जियाँ खरीदीं",
    "ट्रेन दो घंटे देर से आई",
    "हमने पास के रेस्तरां में खाना खाया",
];
/// MEDIUM confidence.
const HI_PAIRS: &[Pair] = &[
    ("किताब", "किताबें", "noun: plural"),
    ("किताब", "किताबों", "noun: oblique pl"),
    ("लड़का", "लड़के", "noun: oblique (subst)"),
    ("पुस्तकालय", "पुस्तकालयों", "noun: oblique pl (long)"),
];

const KA_FILLER: &[&str] = &[
    "ამინდი გუშინ თბილი და მზიანი იყო",
    "ბაზარში ახალი ბოსტნეული ვიყიდე",
    "მატარებელი ორი საათით დაგვიანდა",
];
/// LOW confidence — Georgian paradigms are the ones I am least sure of, and
/// the percentage should be read as indicative only.
const KA_PAIRS: &[Pair] = &[
    ("ბიბლიოთეკა", "ბიბლიოთეკაში", "agglut: locative (long)"),
    ("წიგნი", "წიგნები", "noun: plural"),
    ("ქალაქი", "ქალაქში", "agglut: locative"),
];

const EN_FILLER: &[&str] = &[
    "The weather yesterday was warm and sunny",
    "I bought fresh vegetables at the market",
    "The train arrived two hours late",
    "We had dinner at a place nearby",
];
/// CONTROL — the language whose behaviour is already documented in the code.
const EN_PAIRS: &[Pair] = &[
    ("document", "documentation", "deriv: additive (long)"),
    ("encrypt", "encryption", "deriv: additive"),
    ("child", "children", "noun: additive pl"),
    ("run", "running", "verb: short stem"),
    ("go", "went", "verb: suppletive"),
];

#[test]
fn all_languages() {
    let mut sums: Vec<Summary> = Vec::new();
    for (name, frame, filler, pairs) in [
        (
            "ARABIC (templatic + concatenative)",
            "قرأت عن {} في الجريدة أمس",
            AR_FILLER,
            AR_PAIRS,
        ),
        (
            "PERSIAN (Arabic script, agglutinative)",
            "دیروز درباره {} در روزنامه خواندم",
            AR_FILLER,
            FA_PAIRS,
        ),
        (
            "GREEK (fusional, substituting endings)",
            "Έγραψα μια σημείωση για {} χθες το βράδυ",
            EL_FILLER,
            EL_PAIRS,
        ),
        (
            "RUSSIAN (Cyrillic, 6-case fusional) [high conf]",
            "Я написал заметку про {} вчера вечером",
            RU_FILLER,
            RU_PAIRS,
        ),
        (
            "GERMAN (umlaut plurals + compounding) [high conf]",
            "Ich habe gestern eine Notiz über {} geschrieben",
            DE_FILLER,
            DE_PAIRS,
        ),
        (
            "SPANISH (fusional verbs) [high conf]",
            "Ayer escribí una nota sobre {} por la noche",
            ES_FILLER,
            ES_PAIRS,
        ),
        (
            "TURKISH (agglutinative, purely additive) [high conf]",
            "Dün akşam {} hakkında bir not yazdım",
            TR_FILLER,
            TR_PAIRS,
        ),
        (
            "HEBREW (templatic, FRONT clitics, spaced) [high conf]",
            // NOT `כתבתי` — it contains the query lemma `כתב`, which the
            // padding guard caught. The frame is part of the padding too.
            "רשמתי אתמול בערב הערה על {}",
            HE_FILLER,
            HE_PAIRS,
        ),
        (
            "KOREAN (Hangul, agglutinative) [med conf]",
            "어제 저녁에 {} 관련 메모를 남겼다",
            KO_FILLER,
            KO_PAIRS,
        ),
        (
            "JAPANESE (Kana+Han) [high conf]",
            "昨日の夜、{}についてメモを残した",
            JA_FILLER,
            JA_PAIRS,
        ),
        (
            "CHINESE (isolating) [CONTROL]",
            "昨晚我写了一篇关于{}的笔记",
            ZH_FILLER,
            ZH_PAIRS,
        ),
        (
            "THAI (isolating, unsegmented) [CONTROL]",
            "เมื่อคืนฉันเขียนบันทึกเกี่ยวกับ{}",
            TH_FILLER,
            TH_PAIRS,
        ),
        (
            "HINDI (Devanagari) [med conf]",
            "मैंने कल रात {} के बारे में एक नोट लिखा",
            HI_FILLER,
            HI_PAIRS,
        ),
        (
            "GEORGIAN (agglutinative) [LOW conf]",
            "გუშინ საღამოს {} შესახებ ჩანაწერი გავაკეთე",
            KA_FILLER,
            KA_PAIRS,
        ),
        (
            "ENGLISH [CONTROL]",
            "I wrote a note about {} last night",
            EN_FILLER,
            EN_PAIRS,
        ),
    ] {
        sums.push(run(&Lang {
            name,
            frame,
            filler,
            pairs,
        }));
    }

    // ------------------------------------------------ cross-language table --
    sums.sort_by(|a, b| {
        let (x, y) = (
            a.long_ok as f32 / a.total as f32,
            b.long_ok as f32 / b.total as f32,
        );
        y.partial_cmp(&x).unwrap()
    });
    println!("

=================== ALL LANGUAGES ===================");
    println!(
        "{:<40} {:>7} {:>7} {:>5} | {:>5} {:>5} {:>5} {:>5} {:>5}",
        "language", "1-sent", "~70w", "drop", "flr3", "pfx7", "subs", "skel", "none"
    );
    for s in &sums {
        println!(
            "{:<40} {:>6.1}% {:>6.1}% {:>5} | {:>5} {:>5} {:>5} {:>5} {:>5}",
            s.name,
            100.0 * s.short_ok as f32 / s.total as f32,
            100.0 * s.long_ok as f32 / s.total as f32,
            s.dropped,
            s.floor3,
            s.prefix7,
            s.subseq,
            s.skel_sub,
            s.nothing
        );
    }

    let t: usize = sums.iter().map(|s| s.total).sum();
    let so: usize = sums.iter().map(|s| s.short_ok).sum();
    let lo: usize = sums.iter().map(|s| s.long_ok).sum();
    let dr: usize = sums.iter().map(|s| s.dropped).sum();
    let f3: usize = sums.iter().map(|s| s.floor3).sum();
    let p7: usize = sums.iter().map(|s| s.prefix7).sum();
    let sq: usize = sums.iter().map(|s| s.subseq).sum();
    let sk: usize = sums.iter().map(|s| s.skel_sub).sum();
    let no: usize = sums.iter().map(|s| s.nothing).sum();
    println!("
--- AGGREGATE over {} pairs, {} languages ---", t, sums.len());
    println!("  admitted, 1-sentence drawer : {so}/{t} = {:.1}%", 100.0 * so as f32 / t as f32);
    println!("  admitted, ~70-word drawer   : {lo}/{t} = {:.1}%", 100.0 * lo as f32 / t as f32);
    println!("  DROPPED at realistic length : {dr}/{t} = {:.1}%", 100.0 * dr as f32 / t as f32);
    println!("
  Of those {dr} dropped, one relation change would recover:");
    println!("    containment floor 3       {f3:>3}  ({:.1}% of drops)", 100.0 * f3 as f32 / dr as f32);
    println!("    shared-prefix >=7         {p7:>3}  ({:.1}%)", 100.0 * p7 as f32 / dr as f32);
    println!("    ordered subsequence       {sq:>3}  ({:.1}%)", 100.0 * sq as f32 / dr as f32);
    println!("    skeleton subsequence      {sk:>3}  ({:.1}%)", 100.0 * sk as f32 / dr as f32);
    println!("    reached by NOTHING        {no:>3}  ({:.1}%)", 100.0 * no as f32 / dr as f32);
}
