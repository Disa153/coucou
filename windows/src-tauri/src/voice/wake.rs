// « OK Coucou »: hearing the wake phrase in what Parakeet wrote, and telling
// speech from silence on the microphone. Nothing here touches the OS.

/// Lower case, accents gone, everything but letters and digits a space.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        let base = match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ÿ' | 'ý' => 'y',
            'ñ' => 'n',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };
        out.push(base);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How Parakeet may write "OK".
fn is_ok(word: &str) -> bool {
    matches!(word, "ok" | "okay" | "oke" | "okey" | "hey")
}

/// How Parakeet may write "coucou", in one word.
fn is_coucou(word: &str) -> bool {
    matches!(
        word,
        "coucou" | "coucous" | "cuckoo" | "cucu" | "kuku" | "koukou" | "cookoo" | "coocoo" | "cocou" | "kukuu" | "couscous"
    )
}

/// …or in two.
fn is_coucou_pair(a: &str, b: &str) -> bool {
    matches!((a, b), ("cou", "cou") | ("coo", "coo") | ("ku", "ku") | ("cu", "cu") | ("koo", "koo") | ("cuck", "oo"))
}

/// A word in Latin letters, as it sounds, with no spaces: Cyrillic and Latin
/// spellings of the same words come out close ("Привет Диса" ≈ "privet disa").
fn translit(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars() {
        let s: &str = match c {
            'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' | 'ё' | 'э' => "e", 'ж' => "zh", 'з' => "z",
            'и' | 'й' | 'ы' => "i", 'к' => "k", 'л' => "l", 'м' => "m", 'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r",
            'с' => "s", 'т' => "t", 'у' => "u", 'ф' => "f", 'х' => "h", 'ц' => "ts", 'ч' => "ch", 'ш' => "sh",
            'щ' => "sch", 'ъ' | 'ь' => "", 'ю' => "yu", 'я' => "ya",
            'y' => "i", 'w' => "v", 'c' => "k", 'q' => "k", 'x' => "ks",
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => "",
        };
        out.push_str(s);
    }
    // Doubled letters sound like one ("Дисса" = "Диса").
    let mut squeezed = String::with_capacity(out.len());
    for c in out.chars() {
        if !squeezed.ends_with(c) {
            squeezed.push(c);
        }
    }
    squeezed
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (prev + usize::from(a[i - 1] != b[j - 1])).min(row[j] + 1).min(row[j - 1] + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// When `text` holds `phrase` (the user's wake phrase, e.g. « Привет, Диса »),
/// what was said after it. Close is enough: a letter or two may differ, the
/// words may be glued or split, in Cyrillic or Latin.
pub fn after_phrase(text: &str, phrase: &str) -> Option<String> {
    let target: String = phrase.split_whitespace().map(|w| translit(&normalize(w))).collect();
    if target.chars().count() < 4 {
        return None;
    }
    let allowed = (target.chars().count() + 3) / 5; // about one letter in five
    let phrase_words = phrase.split_whitespace().count().max(1);
    let words: Vec<&str> = text.split_whitespace().collect();
    let latin: Vec<String> = words.iter().map(|w| translit(&normalize(w))).collect();
    let mut best: Option<(usize, usize)> = None; // (distance, end word)
    for start in 0..latin.len() {
        let mut glued = String::new();
        for end in start..latin.len().min(start + phrase_words + 2) {
            glued.push_str(&latin[end]);
            let d = distance(&glued, &target);
            if d <= allowed && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, end));
            }
        }
        if best.is_some() {
            break; // the first place it is said
        }
    }
    let (_, end) = best?;
    let rest = words[end + 1..].join(" ");
    Some(rest.trim_start_matches([',', '.', '!', ' ', '—', '-']).trim().to_string())
}

/// The user's wake phrase, or « OK Coucou ».
pub fn after_any_wake(text: &str, phrase: &str) -> Option<String> {
    after_phrase(text, phrase).or_else(|| after_wake(text))
}

/// When `text` holds the wake phrase, what was said after it (maybe nothing).
/// The words keep their case and punctuation, as Parakeet wrote them.
pub fn after_wake(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let norm: Vec<String> = words.iter().map(|w| normalize(w)).collect();
    // "o.k." and "o k" become two tokens: glue them.
    let mut i = 0;
    while i < norm.len() {
        let mut ok_end = None;
        if is_ok(&norm[i]) {
            ok_end = Some(i + 1);
        } else if norm[i] == "o k" || (norm[i] == "o" && norm.get(i + 1).map(String::as_str) == Some("k")) {
            ok_end = Some(if norm[i] == "o" { i + 2 } else { i + 1 });
        }
        if let Some(j) = ok_end {
            // A word made of two ("ok coucou" written as one token is rare but happens).
            if let Some(w) = norm.get(j) {
                let parts: Vec<&str> = w.split(' ').collect();
                if parts.len() == 1 && is_coucou(parts[0]) {
                    return Some(words[j + 1..].join(" ").trim_start_matches([',', '.', '!', ' ']).trim().to_string());
                }
                if let Some(next) = norm.get(j + 1) {
                    if is_coucou_pair(w, next) {
                        return Some(words[j + 2..].join(" ").trim_start_matches([',', '.', '!', ' ']).trim().to_string());
                    }
                }
            }
        }
        // "okcoucou" in one piece.
        if let Some(rest) = norm[i].strip_prefix("okay").or_else(|| norm[i].strip_prefix("ok")) {
            if is_coucou(rest.trim()) {
                return Some(words[i + 1..].join(" ").trim().to_string());
            }
        }
        i += 1;
    }
    None
}

/// Speech or silence, 30 ms at a time, against a noise floor that follows the
/// room. A stretch of speech is handed out once followed by `hangover` frames
/// of silence, or when it gets too long.
pub struct Vad {
    floor: f32,
    in_speech: bool,
    speech_frames: usize,
    silent_frames: usize,
    frame: Vec<f32>,
    /// The current stretch, with a little audio from before it began.
    current: Vec<f32>,
    preroll: std::collections::VecDeque<f32>,
    /// Silence that ends a stretch, in frames.
    pub hangover: usize,
}

pub const FRAME: usize = 480; // 30 ms at 16 kHz
const PREROLL: usize = 16_000 * 3 / 10;
const MAX_SEGMENT: usize = 16_000 * 12;

impl Default for Vad {
    fn default() -> Self {
        Self {
            floor: 0.004,
            in_speech: false,
            speech_frames: 0,
            silent_frames: 0,
            frame: Vec::with_capacity(FRAME),
            current: Vec::new(),
            preroll: std::collections::VecDeque::with_capacity(PREROLL),
            hangover: 25,
        }
    }
}

impl Vad {
    #[cfg(test)]
    pub fn in_speech(&self) -> bool {
        self.in_speech
    }

    /// Feeds 16 kHz samples; the stretches of speech that ended.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        let mut done = Vec::new();
        for &s in samples {
            self.frame.push(s);
            if self.frame.len() < FRAME {
                continue;
            }
            let rms = (self.frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
            let speech = rms > (self.floor * 3.0).max(0.012);
            if !speech && !self.in_speech {
                self.floor = self.floor * 0.97 + rms * 0.03;
            }
            if self.in_speech {
                self.current.extend_from_slice(&self.frame);
                if speech {
                    self.silent_frames = 0;
                } else {
                    self.silent_frames += 1;
                }
                if self.silent_frames >= self.hangover || self.current.len() >= MAX_SEGMENT {
                    done.push(std::mem::take(&mut self.current));
                    self.in_speech = false;
                    self.speech_frames = 0;
                    self.silent_frames = 0;
                }
            } else if speech {
                self.speech_frames += 1;
                if self.speech_frames >= 3 {
                    self.in_speech = true;
                    self.current = self.preroll.drain(..).collect();
                    self.current.extend_from_slice(&self.frame);
                }
            } else {
                self.speech_frames = 0;
            }
            if !self.in_speech {
                for &x in &self.frame {
                    if self.preroll.len() == PREROLL {
                        self.preroll.pop_front();
                    }
                    self.preroll.push_back(x);
                }
            }
            self.frame.clear();
        }
        done
    }

    /// Forgets what is being heard (Coucou speaks, or dictation starts).
    pub fn reset(&mut self) {
        self.in_speech = false;
        self.speech_frames = 0;
        self.silent_frames = 0;
        self.current.clear();
        self.preroll.clear();
        self.frame.clear();
    }
}

/// "ru-RU" for a Russian answer, "fr-FR" for a French one, else None (the system's language).
pub fn speech_lang(text: &str) -> Option<&'static str> {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let cyrillic = text.chars().filter(|c| ('\u{0400}'..='\u{04FF}').contains(c)).count();
    if cyrillic * 2 > letters.max(1) {
        return Some("ru-RU");
    }
    let n = normalize(text);
    let words: Vec<&str> = n.split(' ').collect();
    let french = ["le", "la", "les", "est", "tu", "vous", "je", "un", "une", "des", "pour", "pas", "c", "et", "sur", "avec", "ton", "ta"];
    let count = words.iter().filter(|w| french.contains(w)).count();
    (count * 6 >= words.len().max(1) && count >= 1).then_some("fr-FR")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wake_phrase_however_parakeet_writes_it() {
        assert_eq!(after_wake("OK Coucou, play some music.").as_deref(), Some("play some music."));
        assert_eq!(after_wake("Okay, coucou! Quelle heure est-il ?").as_deref(), Some("Quelle heure est-il ?"));
        assert_eq!(after_wake("O.K. Cuckoo what's the weather").as_deref(), Some("what's the weather"));
        assert_eq!(after_wake("ok cou cou ajoute Stripe").as_deref(), Some("ajoute Stripe"));
        assert_eq!(after_wake("OK Coucou.").as_deref(), Some(""));
        assert_eq!(after_wake("Hey coucou, ça va ?").as_deref(), Some("ça va ?"));
        assert_eq!(after_wake("on a dit coucou à Paul"), None);
        assert_eq!(after_wake("ok let's go"), None);
        assert_eq!(normalize("Ça VA, Élodie ?"), "ca va elodie");
    }

    #[test]
    fn privet_disa_however_parakeet_writes_it() {
        let p = "Привет, Диса";
        assert_eq!(after_phrase("Привет, Диса, открой чат", p).as_deref(), Some("открой чат"));
        assert_eq!(after_phrase("привет диса что делает клод", p).as_deref(), Some("что делает клод"));
        assert_eq!(after_phrase("Привет, Дисса! Запусти тесты.", p).as_deref(), Some("Запусти тесты."));
        assert_eq!(after_phrase("Приветдиса сделай коммит", p).as_deref(), Some("сделай коммит"));
        assert_eq!(after_phrase("Privet Disa, open the chat", p).as_deref(), Some("open the chat"));
        assert_eq!(after_phrase("Превет, Диса.", p).as_deref(), Some(""));
        assert_eq!(after_phrase("Ну, привет, Диса, как дела", p).as_deref(), Some("как дела"));
        assert_eq!(after_phrase("привет как дела", p), None);
        assert_eq!(after_phrase("Диса привет", p), None);
        assert_eq!(after_phrase("сегодня хорошая погода", p), None);
        // Another phrase the user typed.
        assert_eq!(after_phrase("Сяо Чоу Чоу, запусти тесты", "Сяо Чоу Чоу").as_deref(), Some("запусти тесты"));
        assert_eq!(after_phrase("anything", "ок"), None, "too short to be safe");
        assert_eq!(after_any_wake("OK Coucou, play music", p).as_deref(), Some("play music"));
    }

    #[test]
    fn speech_is_cut_after_a_pause() {
        let mut vad = Vad::default();
        let silence = vec![0.0f32; 16_000];
        let tone: Vec<f32> = (0..16_000).map(|i| (i as f32 * 0.1).sin() * 0.3).collect();
        assert!(vad.feed(&silence).is_empty());
        assert!(vad.feed(&tone).is_empty());
        assert!(vad.in_speech());
        let out = vad.feed(&silence);
        assert_eq!(out.len(), 1);
        // The tone, a pre-roll before it and the hangover after it.
        assert!(out[0].len() > 16_000 && out[0].len() <= 16_000 + 25 * FRAME + PREROLL + 2 * FRAME);
        assert!(!vad.in_speech());
    }

    #[test]
    fn french_answers_are_spoken_in_french() {
        assert_eq!(speech_lang("C'est fait, j'ai ajouté Stripe pour toi."), Some("fr-FR"));
        assert_eq!(speech_lang("Done, Stripe is in the island."), None);
        assert_eq!(speech_lang("Готово, тесты запущены."), Some("ru-RU"));
    }
}
