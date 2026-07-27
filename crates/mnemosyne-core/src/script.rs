//! Script-aware segmentation for comparison keys.
//!
//! Splitting on `!char::is_alphanumeric()` finds word boundaries only in
//! scripts that mark them. It does not in Han, Kana, Hangul, Bopomofo,
//! Arabic, Khmer, Thai, Lao or Myanmar — there a whole clause collapses into
//! a single token, and a query for a word the drawer visibly contains matches
//! nothing at all:
//!
//! ```text
//! doc   我昨天去了北京参加会议  -> ["我昨天去了北京参加会议"]
//! query 北京                  -> ["北京"]          0 / 1 matched
//! ```
//!
//! That is not a ranking problem. `search` drops any hit with no lexical
//! evidence and a merely neutral cosine, so the observable is an empty result
//! set that reads as an empty vault.
//!
//! Note this is *not* the same failure everywhere. Han and Kana produce one
//! stable mega-token, so at least both sides agree. Khmer, Thai and Myanmar
//! carry marks that are combining but **not** `Other_Alphabetic` (Khmer COENG
//! U+17D2, Thai tone marks U+0E48..U+0E4C, Myanmar ASAT U+103A), which *do*
//! split — into fragments that start and end mid-word, positioned by whatever
//! word happens to follow. Query and document then disagree, which is
//! strictly worse: the same Thai word matches when it ends the document and
//! misses when it begins it.
//!
//! The fix is to segment such runs into character bigrams (Lucene's
//! `CJKBigramFilter` shape), symmetrically on both sides — plus unigrams, but
//! **only where a character is a word**. That distinction is load-bearing in
//! both directions. Without Han unigrams, `好` in `他说：「好。」` is a working
//! token today that the change would delete. With unigrams everywhere,
//! `قطار` matches `المستشفى` on a shared alef, and since a hit is admitted on
//! `lexical > 0.0`, that does not just add noise — it retires the relevance
//! gate for every query in the script.
//!
//! Two boundaries this deliberately does not cross:
//!
//! * **Runs are split by script first.** `我们用Kubernetes部署` bigrammed
//!   whole would emit `wi, in, nd, do, ow, ws` and destroy an exact brand-name
//!   match that works today. Latin and digit subruns are left whole.
//! * **Delimiting scripts are untouched.** Georgian, Greek, Cyrillic, Latin
//!   and Tibetan (which delimits on the tsheg U+0F0B) mark their boundaries;
//!   their remaining defects are folding and morphology, not segmentation, and
//!   n-grams are the wrong tool for those.

/// The scripts this module treats specially, plus `Other` for everything that
/// already marks its own word boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Han,
    Hiragana,
    Katakana,
    Hangul,
    Bopomofo,
    Arabic,
    Khmer,
    Thai,
    Lao,
    Myanmar,
    /// Any script that delimits its words — Latin, Cyrillic, Greek, Georgian,
    /// Tibetan, Devanagari, digits, and everything else.
    Other,
}

impl Script {
    /// True when the script attaches words or morphology without a delimiter,
    /// so a boundary split cannot find word edges inside it.
    ///
    /// Arabic is included even though it spaces its words: the definite
    /// article and the conjunction/preposition proclitics attach with no
    /// delimiter, so `كتاب` cannot reach `الكتاب` by any split.
    pub fn attaches_without_delimiter(self) -> bool {
        !matches!(self, Script::Other)
    }

    /// True when a single character is itself a unit of meaning, so emitting
    /// it alone is evidence rather than noise.
    ///
    /// This is what separates Han from the rest. `好` and `猫` are words; a
    /// lone Arabic `ا`, Thai consonant or Korean syllable is not. Emitting
    /// unigrams for the alphabetic scripts makes any two texts in the script
    /// share a token — `قطار` would "match" `المستشفى` on the alef — and
    /// since `search` admits a hit on `lexical > 0.0`, that does not merely
    /// add noise to the ranking, it defeats the relevance gate entirely.
    pub fn is_logographic(self) -> bool {
        matches!(self, Script::Han)
    }
}

/// Classify a character by script.
///
/// Explicit ranges rather than a Unicode Script table dependency: the set we
/// treat specially is small, fixed, and each range below is the reason it is
/// here. Everything unlisted is `Other`, which is the safe answer — it means
/// "leave this alone".
pub fn script_of(c: char) -> Script {
    match c as u32 {
        // Han. Includes the CJK iteration mark 々 (U+3005), which repeats the
        // preceding ideograph and so belongs to its run.
        0x3005 | 0x303B => Script::Han,
        0x3400..=0x4DBF        // Extension A
        | 0x4E00..=0x9FFF      // Unified Ideographs
        | 0xF900..=0xFAFF      // Compatibility Ideographs
        | 0x20000..=0x2A6DF    // Extension B
        | 0x2A700..=0x2EBEF    // Extensions C-F
        | 0x2F800..=0x2FA1F => Script::Han,

        0x3040..=0x309F => Script::Hiragana,
        // Katakana, phonetic extensions, and halfwidth forms. The prolonged
        // sound mark ー (U+30FC) lives in this block and belongs to its run.
        0x30A0..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Script::Katakana,

        0x1100..=0x11FF        // Jamo
        | 0x3130..=0x318F      // Compatibility Jamo
        | 0xA960..=0xA97F      // Jamo Extended-A
        | 0xAC00..=0xD7A3      // Syllables
        | 0xD7B0..=0xD7FF => Script::Hangul,

        0x3105..=0x312F | 0x31A0..=0x31BF => Script::Bopomofo,

        0x0600..=0x06FF        // Arabic
        | 0x0750..=0x077F      // Supplement
        | 0x08A0..=0x08FF      // Extended-A
        | 0xFB50..=0xFDFF      // Presentation Forms-A
        | 0xFE70..=0xFEFF => Script::Arabic,

        0x1780..=0x17FF | 0x19E0..=0x19FF => Script::Khmer,
        0x0E00..=0x0E7F => Script::Thai,
        0x0E80..=0x0EFF => Script::Lao,
        0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F => Script::Myanmar,

        _ => Script::Other,
    }
}

/// The result of segmenting a text into comparison tokens.
pub struct Segmented {
    /// Tokens for term matching. Both the query and the document side go
    /// through this, so matching stays symmetric.
    /// Every token, **unfiltered**. Callers apply their own minimum-length
    /// rule: BM25 keeps the historical `len() > 1` *byte* test, while the
    /// hash embedder has always indexed one-letter words and must keep doing
    /// so or `vitamin C` stops being distinguishable from `vitamin D`.
    pub tokens: Vec<String>,
    /// Content units, counting a segmented run **once per character** rather
    /// than once per emitted n-gram.
    ///
    /// BM25 divides by document length. Without this, segmenting a run into
    /// unigrams plus bigrams would roughly triple the measured length of
    /// exactly the documents the segmentation exists to serve, and length
    /// normalization would take back most of the gain.
    pub len: usize,
}

/// Segment `text` into comparison tokens.
///
/// `text` is expected to be already canonicalized and lowercased by the
/// caller — this function decides boundaries, not encoding.
pub fn segment(text: &str) -> Segmented {
    let mut out = Segmented {
        tokens: Vec::new(),
        len: 0,
    };
    for run in text.split(|c: char| !c.is_alphanumeric()) {
        if run.is_empty() {
            continue;
        }
        let mut start = 0usize;
        let mut current: Option<Script> = None;
        for (offset, ch) in run.char_indices() {
            let script = script_of(ch);
            match current {
                None => {
                    current = Some(script);
                    start = offset;
                }
                Some(prev) if prev == script => {}
                Some(prev) => {
                    emit(&mut out, &run[start..offset], prev);
                    current = Some(script);
                    start = offset;
                }
            }
        }
        if let Some(prev) = current {
            emit(&mut out, &run[start..], prev);
        }
    }
    out
}

/// Emit one same-script subrun.
fn emit(out: &mut Segmented, sub: &str, script: Script) {
    if sub.is_empty() {
        return;
    }
    if !script.attaches_without_delimiter() {
        out.len += 1;
        out.tokens.push(sub.to_string());
        return;
    }
    let chars: Vec<char> = sub.chars().collect();
    out.len += chars.len();
    // Unigrams only where a character is a word (see `is_logographic`).
    if script.is_logographic() {
        for ch in &chars {
            out.tokens.push(ch.to_string());
        }
    }
    if chars.len() < 2 {
        // A one-character subrun bounded by delimiters is a real token in any
        // script, and was one before this change — a lone Arabic letter is
        // 2 bytes and cleared the old byte filter.
        if !script.is_logographic() {
            out.tokens.push(sub.to_string());
        }
        return;
    }
    for pair in chars.windows(2) {
        out.tokens.push(format!("{}{}", pair[0], pair[1]));
    }
    // The whole unit, when it is longer than the bigrams already emitted. For
    // Arabic this is the word itself and carries the strongest signal; for a
    // CJK run it is the clause, which is what the old tokenizer produced, so
    // nothing that matched before stops matching.
    if chars.len() > 2 {
        out.tokens.push(sub.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors `mnemosyne_store::tokenize` exactly: the retrieval fold, then
    /// segmentation, then the historical byte filter. Using bare
    /// `to_lowercase` here instead would let these tests pass while the
    /// product behaved differently.
    fn toks(s: &str) -> Vec<String> {
        segment(&crate::normalize::search_key(s))
            .tokens
            .into_iter()
            .filter(|t| t.len() > 1)
            .collect()
    }

    /// How many of the query's tokens the document supplies. Zero is the
    /// failure this module exists to remove.
    fn matched(query: &str, doc: &str) -> usize {
        let d = toks(doc);
        toks(query).iter().filter(|t| d.contains(t)).count()
    }

    #[test]
    fn a_chinese_clause_is_no_longer_one_token() {
        let t = toks("我昨天去了北京参加会议");
        assert!(t.contains(&"北京".to_string()), "{t:?}");
        assert!(t.contains(&"北".to_string()));
        // The old whole-clause token survives, so nothing that matched before stops.
        assert!(t.contains(&"我昨天去了北京参加会议".to_string()));
    }

    #[test]
    fn the_city_in_the_sentence_is_findable() {
        assert_eq!(matched("北京", "我昨天去了北京参加会议"), 3); // 北, 京, 北京
        assert_eq!(matched("東京", "昨日は東京で会議に参加しました"), 3);
    }

    /// The failure that is worse than Chinese: fragments positioned by the
    /// following word, so the same word matches at one end and not the other.
    #[test]
    fn a_thai_word_matches_wherever_it_sits() {
        assert!(matched("ประชุม", "ประชุมทีมงานที่กรุงเทพ") > 0);
        assert!(matched("กรุงเทพ", "ประชุมทีมงานที่กรุงเทพ") > 0);
        assert!(matched("ประชุม", "ฉันไปกรุงเทพเมื่อวานนี้เพื่อเข้าร่วมประชุม") > 0);
    }

    #[test]
    fn khmer_coeng_no_longer_shatters_the_word() {
        let doc = "ខ្ញុំបានទៅភ្នំពេញកាលពីម្សិលមិញដើម្បីចូលរួមប្រជុំ";
        assert!(matched("ភ្នំពេញ", doc) > 0);
        assert!(matched("ចូលរួម", doc) > 0);
    }

    /// Arabic spaces its words; what attaches with no delimiter is the
    /// definite article and the proclitics.
    #[test]
    fn arabic_reaches_through_the_definite_article() {
        assert!(matched("كتاب", "قرأت الكتاب أمس") > 0);
        assert!(matched("مكتبة", "ذهبت إلى المكتبة") > 0);
        // A three-letter root, which the hash embedder cannot reach at all.
        assert!(matched("بيت", "دخلت البيت") > 0);
    }

    /// Different words must stay different, or the fix is just noise.
    ///
    /// The Arabic pair is the one that caught it: with unigrams in every
    /// script, these two share the alef and the drawer clears the relevance
    /// gate on a single letter.
    #[test]
    fn unrelated_terms_still_do_not_match() {
        assert_eq!(matched("قطار", "ذهبت إلى المستشفى"), 0);
        assert_eq!(matched("東京", "私は大阪に住んでいます"), 0);
        assert_eq!(matched("ประชุม", "ฉันชอบอาหารไทย"), 0);
    }

    /// No alphabetic non-delimiting script may emit a bare character.
    #[test]
    fn only_logographic_scripts_emit_single_characters() {
        assert!(toks("الكتاب").iter().all(|t| t.chars().count() > 1));
        assert!(toks("한국어는").iter().all(|t| t.chars().count() > 1));
        assert!(toks("กรุงเทพ").iter().all(|t| t.chars().count() > 1));
        // Han does, because there a character is a word.
        assert!(toks("北京参加会议").contains(&"北".to_string()));
    }

    #[test]
    fn korean_reaches_through_its_particles() {
        assert!(matched("한국어", "한국어는 어렵다") > 0);
        assert!(matched("서울", "어제 서울에서 회의에 참석했습니다") > 0);
    }

    /// A Latin brand name inside CJK must not be shredded into bigrams.
    #[test]
    fn latin_subruns_are_left_whole() {
        let t = toks("我们用Kubernetes部署");
        assert!(t.contains(&"kubernetes".to_string()), "{t:?}");
        assert!(!t.contains(&"wi".to_string()), "{t:?}");
        assert!(t.contains(&"部署".to_string()));
    }

    /// Japanese splits kanji from kana, so grammatical particles do not glue
    /// themselves to content words.
    #[test]
    fn japanese_splits_at_script_boundaries() {
        let t = toks("東京タワー");
        assert!(t.contains(&"東京".to_string()), "{t:?}");
        assert!(t.contains(&"タワー".to_string()), "{t:?}");
        // The two scripts must not produce a bigram straddling the boundary.
        assert!(!t.contains(&"京タ".to_string()), "{t:?}");
    }

    /// A single ideograph is a real word and must survive.
    #[test]
    fn a_lone_ideograph_is_a_token() {
        assert!(toks("他说：「好。」").contains(&"好".to_string()));
        assert!(matched("好", "他说：「好。」") > 0);
    }

    #[test]
    fn latin_cyrillic_and_georgian_tokenize_exactly_as_before() {
        // The regression guard: English, Russian and Georgian must stay
        // byte-identical to the old split-on-non-alphanumeric behaviour.
        // Note the single-letter Cyrillic words: the historical filter is a
        // *byte* test, so `я` and `в` (2 bytes each) always survived it while
        // a one-letter English word does not. That asymmetry is preserved
        // here deliberately — changing it would silently drop tokens from
        // every existing vault.
        //
        // Greek is deliberately gone from this list: its tonos folds now, so
        // `Αθήνα` yields `αθηνα`. These three rows carry no ё, no loose acute
        // and rely on Mkhedruli being caseless, which makes the test a
        // tripwire for any *future* Cyrillic or Georgian fold.
        let cases = [
            (
                "I went to Beijing yesterday",
                vec!["went", "to", "beijing", "yesterday"],
            ),
            ("Я поехал в Москву", vec!["я", "поехал", "в", "москву"]),
            ("წიგნი მაგიდაზე", vec!["წიგნი", "მაგიდაზე"]),
        ];
        for (input, want) in cases {
            let got = toks(input);
            let want: Vec<String> = want.into_iter().map(str::to_string).collect();
            assert_eq!(got, want, "input: {input}");
        }
    }

    /// Tibetan delimits on the tsheg, so it needs no segmentation and must
    /// not get any.
    #[test]
    fn tibetan_is_left_alone() {
        assert_eq!(script_of('\u{0F40}'), Script::Other);
    }

    /// `segment` itself filters nothing — a one-letter word reaches the
    /// caller, and only BM25 drops it. The hash embedder needs it.
    #[test]
    fn one_letter_words_reach_the_caller() {
        let all = segment("vitamin c").tokens;
        assert!(all.contains(&"c".to_string()), "{all:?}");
        // ...and BM25's byte filter is what removes it, as it always has.
        assert!(!toks("vitamin c").contains(&"c".to_string()));
    }

    /// Length must count content, not the n-gram expansion, or BM25's length
    /// normalization penalises exactly the documents this serves.
    #[test]
    fn length_counts_characters_not_ngrams() {
        let s = segment("北京");
        assert_eq!(s.len, 2);
        assert!(s.tokens.len() > 2, "{:?}", s.tokens);

        // An 11-character clause counts as 11, comparable to 11 words.
        assert_eq!(segment("我昨天去了北京参加会议").len, 11);
        // Latin is unchanged: one unit per word.
        assert_eq!(segment("i went to beijing").len, 4);
    }

    /// Was a documented gap, now closed: Turkish dotted capital İ lowercases
    /// to `i` + U+0307, which is combining but not Other_Alphabetic, so it
    /// split the word and the byte filter ate the fragments — `İZMİR` gave
    /// `["zmi"]`. `search_key` strips the mark after lowercasing, which needs
    /// no Turkic tailoring and so keeps Turkish ı/i minimal pairs.
    #[test]
    fn turkish_dotted_capital_folds_to_izmir() {
        assert_eq!(toks("İZMİR"), vec!["izmir".to_string()]);
        assert_eq!(toks("İzmir"), toks("izmir"));
    }
}
