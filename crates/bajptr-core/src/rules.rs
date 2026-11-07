use crate::fields::MAX_STRING;
use crate::{Error, Result, invalid};
use fancy_regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Rule {
    pub old: String,
    pub new: String,
    pub mode: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub table: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Miss {
    pub index: usize,
    pub reason: String,
    pub example: String,
    pub table: String,
}
#[derive(Default)]
pub struct Evidence {
    pub hits: Vec<u64>,
    pub misses: Vec<Option<Miss>>,
    pub explain: bool,
}
impl Evidence {
    pub fn new(count: usize, explain: bool) -> Self {
        Self {
            hits: vec![0; count],
            misses: vec![None; count],
            explain,
        }
    }
    fn miss(&mut self, index: usize, reason: &str, text: &str, table: &str) {
        if self.explain && self.misses[index].is_none() {
            self.misses[index] = Some(Miss {
                index,
                reason: reason.into(),
                example: text.chars().take(500).collect(),
                table: table.into(),
            });
        }
    }
    pub fn zero_reasons(&self) -> Vec<Miss> {
        self.hits
            .iter()
            .enumerate()
            .filter(|(_, v)| **v == 0)
            .map(|(index, _)| {
                self.misses[index].clone().unwrap_or(Miss {
                    index,
                    reason: "在所选来源中没有找到符合匹配方式的原文。可在“查找”核对原词和来源。"
                        .into(),
                    example: String::new(),
                    table: String::new(),
                })
            })
            .collect()
    }
}

#[derive(Clone)]
enum Part {
    Literal(String),
    Group(usize),
}
struct PortableRegex {
    pattern: Regex,
    replacement: Vec<Part>,
    nonempty: std::sync::OnceLock<Option<Regex>>,
}
impl PortableRegex {
    fn new(old: &str, new: &str) -> Result<Self> {
        if old.chars().count() > 16384 {
            return Err(invalid("正则表达式过长，请拆成几条词条。"));
        }
        let unsupported = || invalid("表达式或替换内容无效，请检查分组或使用正则模板。");
        let translated = translate_pattern(old)?;
        let pattern = RegexBuilder::new(&translated)
            .backtrack_limit(100_000)
            .build()
            .map_err(|e| invalid(format!("表达式无效：{e}")))?;
        let mut replacement = Vec::new();
        let mut literal = String::new();
        let mut chars = new.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\\' {
                literal.push(c);
                continue;
            }
            let next = chars.next().ok_or_else(unsupported)?;
            let group = if next == 'g' {
                if chars.next() != Some('<') {
                    return Err(unsupported());
                }
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('>') => break,
                        Some(v) => name.push(v),
                        None => return Err(unsupported()),
                    }
                }
                Some(if let Ok(index) = name.parse::<usize>() {
                    index
                } else {
                    pattern
                        .capture_names()
                        .position(|v| v == Some(name.as_str()))
                        .ok_or_else(unsupported)?
                })
            } else if next == '0'
                || (matches!(next, '1'..='7')
                    && chars.clone().take(2).count() == 2
                    && chars.clone().take(2).all(|c| matches!(c, '0'..='7')))
            {
                let mut digits = String::from(next);
                for _ in 0..2 {
                    if chars.peek().is_some_and(|c| matches!(c, '0'..='7')) {
                        digits.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                let code = u32::from_str_radix(&digits, 8).map_err(|_| unsupported())?;
                if code > 255 {
                    return Err(unsupported());
                }
                literal.push(char::from_u32(code).unwrap());
                None
            } else if matches!(next, '1'..='9') {
                let mut number = String::from(next);
                if chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                    number.push(chars.next().unwrap());
                }
                Some(number.parse::<usize>().map_err(|_| unsupported())?)
            } else {
                let escaped = match next {
                    '\\' => Some('\\'),
                    'n' => Some('\n'),
                    'r' => Some('\r'),
                    't' => Some('\t'),
                    'f' => Some('\x0c'),
                    'a' => Some('\x07'),
                    'v' => Some('\x0b'),
                    'b' => Some('\x08'),
                    _ => None,
                };
                if let Some(c) = escaped {
                    literal.push(c);
                } else if next.is_ascii_alphabetic() {
                    return Err(unsupported());
                } else {
                    literal.push('\\');
                    literal.push(next);
                }
                None
            };
            if let Some(group) = group {
                if group >= pattern.captures_len() {
                    return Err(unsupported());
                }
                if !literal.is_empty() {
                    replacement.push(Part::Literal(std::mem::take(&mut literal)));
                }
                replacement.push(Part::Group(group));
            }
        }
        if !literal.is_empty() {
            replacement.push(Part::Literal(literal));
        }
        Ok(Self {
            pattern,
            replacement,
            nonempty: std::sync::OnceLock::new(),
        })
    }
    fn for_each(
        &self,
        text: &str,
        mut accept: impl FnMut(&fancy_regex::Captures<'_>) -> Result<()>,
    ) -> Result<()> {
        let started = std::time::Instant::now();
        let mut pos = 0;
        let mut matches = 0;
        while pos <= text.len() {
            let Some(caps) = self
                .pattern
                .captures_from_pos(text, pos)
                .map_err(regex_error)?
            else {
                break;
            };
            let m = caps.get(0).unwrap();
            let start = m.start();
            let end = m.end();
            accept(&caps)?;
            matches += 1;
            if matches % 32 == 0 && started.elapsed() > std::time::Duration::from_millis(100) {
                return Err(Error::Regex(
                    "正则匹配超过单段处理时间上限，请简化表达式；未发布补丁。".into(),
                ));
            }
            if start != end {
                pos = end;
                continue;
            }
            // Python VERSION0 permits a nonempty match at the same position just
            // after an empty match. fancy-regex's iterator skips that candidate.
            let alternate = self.nonempty.get_or_init(|| {
                RegexBuilder::new(self.pattern.as_str())
                    .backtrack_limit(100_000)
                    .find_not_empty(true)
                    .build()
                    .ok()
            });
            if let Some(alternate) = alternate
                && let Some(next) = alternate
                    .captures_from_pos(text, end)
                    .map_err(regex_error)?
            {
                let m = next.get(0).unwrap();
                if m.start() == end {
                    accept(&next)?;
                    pos = m.end();
                    continue;
                }
            }
            if end == text.len() {
                break;
            }
            pos = end + text[end..].chars().next().unwrap().len_utf8();
        }
        Ok(())
    }
    fn expand(&self, captures: &fancy_regex::Captures<'_>) -> Result<String> {
        let mut text = String::new();
        for part in &self.replacement {
            match part {
                Part::Literal(value) => {
                    if text.len() + value.len() > MAX_STRING {
                        return Err(invalid("替换结果过长，请缩短替换文字。"));
                    }
                    text.push_str(value)
                }
                Part::Group(index) => {
                    if let Some(m) = captures.get(*index) {
                        if text.len() + m.as_str().len() > MAX_STRING {
                            return Err(invalid("替换结果过长，请缩短替换文字。"));
                        }
                        text.push_str(m.as_str())
                    }
                }
            }
        }
        Ok(text)
    }
}

fn regex_error(error: fancy_regex::Error) -> Error {
    Error::Regex(format!(
        "正则处理已停止：{error}。请缩短表达式或使用模板；未发布补丁。"
    ))
}
/// Translate Python re character classes/anchors instead of inheriting Rust's
/// broader Unicode word definition (combining marks differ).
fn translate_pattern(source: &str) -> Result<String> {
    #[derive(Clone, Copy, Default)]
    struct Flags {
        ascii: bool,
        multiline: bool,
        insensitive: bool,
        verbose: bool,
    }
    let mut flags = Flags::default();
    let mut stack = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let mut at = 0;
    let mut class = false;
    let mut class_first = false;
    let mut out = String::new();
    let mut class_start = 0;
    while at < chars.len() {
        let c = chars[at];
        at += 1;
        if c == '\\' {
            let n = *chars
                .get(at)
                .ok_or_else(|| invalid("表达式以未完成的反斜线结尾。"))?;
            at += 1;
            match n {
                'Z' => out.push_str(r"\z"),
                'w' | 'W' | 's' | 'S' | 'd' | 'D' if flags.ascii => {
                    let body = match n.to_ascii_lowercase() {
                        'w' => "a-zA-Z0-9_",
                        's' => r"\t\n\r\x0b\x0c ",
                        _ => "0-9",
                    };
                    if n.is_ascii_uppercase() {
                        out.push_str(&format!("[^{body}]"));
                    } else if class {
                        out.push_str(body);
                    } else {
                        out.push_str(&format!("[{body}]"));
                    }
                }
                'b' | 'B' if !class && flags.ascii => {
                    let w = "[a-zA-Z0-9_]";
                    out.push_str(&if n == 'b' {
                        format!("(?:(?<={w})(?!{w})|(?<!{w})(?={w}))")
                    } else {
                        format!("(?:(?<={w})(?={w})|(?<!{w})(?!{w}))")
                    });
                }
                'b' if class => out.push_str(r"\x08"),
                'N' => {
                    return Err(invalid(
                        "暂不支持按 Unicode 字符名匹配，请把 \\N{…} 换成对应文字。",
                    ));
                }
                'p' | 'P' | 'K' | 'G' | 'X' | 'R' | 'h' | 'H' | 'k' | 'g' => {
                    return Err(invalid(
                        "表达式包含非标准 re 转义，请检查表达式或使用模板。",
                    ));
                }
                'U' => {
                    let digits: String = chars.iter().skip(at).take(8).collect();
                    if digits.len() != 8 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err(invalid("Unicode 转义无效。"));
                    }
                    out.push_str(&format!(r"\x{{{digits}}}"));
                    at += 8;
                }
                _ => {
                    out.push('\\');
                    out.push(n);
                }
            }
            class_first = false;
            continue;
        }
        if class {
            if c == ']' && !class_first {
                class = false;
                out.push(c);
                if flags.insensitive {
                    let expanded = fold_class(&out[class_start..], flags.ascii)?;
                    out.truncate(class_start);
                    out.push_str(&expanded);
                }
            } else {
                if matches!(c, '[' | '&' | '~' | '|') || c == ']' {
                    out.push('\\');
                }
                out.push(c);
                if c != '^' || !class_first {
                    class_first = false;
                }
            }
            continue;
        }
        if c == '[' {
            class = true;
            class_first = true;
            class_start = out.len();
            out.push(c);
            continue;
        }
        if c == '#' && flags.verbose {
            out.push('#');
            out.extend(&chars[at..]);
            break;
        }
        if c == '(' {
            if chars.get(at) == Some(&'?') {
                let mut end = at + 1;
                while chars.get(end).is_some_and(|c| "aimsux-".contains(*c)) {
                    end += 1;
                }
                if end > at + 1 && matches!(chars.get(end), Some(':' | ')')) {
                    let old = flags;
                    let mut negative = false;
                    let mut keep = String::new();
                    for flag in &chars[at + 1..end] {
                        match flag {
                            '-' => {
                                negative = true;
                                keep.push('-');
                            }
                            'a' => flags.ascii = !negative,
                            'u' => flags.ascii = false,
                            'm' => {
                                flags.multiline = !negative;
                                keep.push(*flag);
                            }
                            'i' => {
                                flags.insensitive = !negative;
                                keep.push(*flag);
                            }
                            'x' => {
                                flags.verbose = !negative;
                                keep.push(*flag);
                            }
                            _ => keep.push(*flag),
                        }
                    }
                    if keep.ends_with('-') {
                        keep.pop();
                    }
                    if chars[end] == ':' {
                        stack.push(old);
                        if keep.is_empty() {
                            out.push_str("(?:");
                        } else {
                            out.push_str(&format!("(?{keep}:"));
                        }
                    } else if !keep.is_empty() {
                        out.push_str(&format!("(?{keep})"));
                    }
                    at = end + 1;
                    continue;
                }
                if chars.get(at + 1) == Some(&'#') {
                    let start = at - 1;
                    while at < chars.len() && chars[at] != ')' {
                        at += 1;
                    }
                    if at < chars.len() {
                        at += 1;
                    }
                    out.extend(&chars[start..at]);
                    continue;
                }
            }
            stack.push(flags);
            out.push(c);
            // Copy group names/condition headers as syntax, not literal text.
            if chars.get(at) == Some(&'?') {
                let rest: String = chars[at..].iter().collect();
                let terminator = if rest.starts_with("?P<") {
                    Some('>')
                } else if rest.starts_with("?P=") || rest.starts_with("?(") {
                    Some(')')
                } else {
                    None
                };
                if let Some(end) = terminator {
                    while at < chars.len() {
                        let x = chars[at];
                        out.push(x);
                        at += 1;
                        if x == end {
                            break;
                        }
                    }
                    if rest.starts_with("?P=") {
                        stack.pop();
                    }
                }
            }
        } else if c == ')' {
            if let Some(previous) = stack.pop() {
                flags = previous;
            }
            out.push(c);
        } else if c == '$' {
            out.push_str(if flags.multiline {
                r"(?=\n|\z)"
            } else {
                r"(?=\n?\z)"
            });
        } else if flags.insensitive {
            out.push_str(&fold_literal(c, flags.ascii));
        } else {
            out.push(c);
        }
    }
    Ok(out)
}
fn fold_literal(c: char, ascii: bool) -> String {
    if ascii && c.is_ascii_alphabetic() {
        return format!(
            "(?-i:[{}{}])",
            c.to_ascii_lowercase(),
            c.to_ascii_uppercase()
        );
    }
    if ascii && c.is_alphabetic() {
        return format!("(?-i:{c})");
    }
    match c {
        'i' => "(?-i:[iIİ])".into(),
        'I' => "(?-i:[iIı])".into(),
        'İ' => "(?-i:[iİ])".into(),
        'ı' => "(?-i:[Iı])".into(),
        _ => c.to_string(),
    }
}
fn fold_class(source: &str, ascii: bool) -> Result<String> {
    use regex_syntax::hir::{Class, ClassUnicode, ClassUnicodeRange, HirKind};
    let negative = source.starts_with("[^");
    let positive = if negative {
        format!("[{}", &source[2..])
    } else {
        source.into()
    };
    let hir = regex_syntax::Parser::new()
        .parse(&positive)
        .map_err(|e| invalid(format!("字符范围无效：{e}")))?;
    let mut class = match hir.kind() {
        HirKind::Class(Class::Unicode(c)) => c.clone(),
        HirKind::Literal(v) => {
            let t = std::str::from_utf8(&v.0).map_err(|_| invalid("字符范围无效。"))?;
            ClassUnicode::new(t.chars().map(|c| ClassUnicodeRange::new(c, c)))
        }
        _ => return Err(invalid("字符范围无效。")),
    };
    let has = |c| {
        class
            .ranges()
            .iter()
            .any(|r| r.start() <= c && c <= r.end())
    };
    if ascii {
        let extra = ('a'..='z')
            .filter(|c| has(*c) || has(c.to_ascii_uppercase()))
            .flat_map(|c| [c, c.to_ascii_uppercase()])
            .map(|c| ClassUnicodeRange::new(c, c))
            .collect::<Vec<_>>();
        class.union(&ClassUnicode::new(extra));
    } else {
        let mut special = Vec::new();
        for (c, equivalent) in [('i', "iIİ"), ('I', "iIı"), ('İ', "iİ"), ('ı', "Iı")] {
            if has(c) {
                special.extend(equivalent.chars().map(|c| ClassUnicodeRange::new(c, c)));
            }
        }
        class.case_fold_simple();
        class.difference(&ClassUnicode::new(
            "iIİı".chars().map(|c| ClassUnicodeRange::new(c, c)),
        ));
        class.union(&ClassUnicode::new(special));
    }
    if negative {
        class.negate();
    }
    let mut out = String::from("(?-i:[");
    for range in class.ranges() {
        out.push_str(&format!(
            r"\x{{{:x}}}-\x{{{:x}}}",
            range.start() as u32,
            range.end() as u32
        ));
    }
    out.push_str("])");
    Ok(out)
}

pub fn regex_sample(old: &str, new: &str, text: &str) -> Result<(String, usize)> {
    let re = PortableRegex::new(old, new)?;
    let mut out = String::new();
    let mut cursor = 0;
    let mut count = 0;
    re.for_each(text, |caps| {
        let m = caps.get(0).unwrap();
        out.push_str(&text[cursor..m.start()]);
        out.push_str(&re.expand(caps)?);
        cursor = m.end();
        count += 1;
        if out.len() > MAX_STRING {
            return Err(invalid("试写结果过长，请缩短替换文字。"));
        }
        Ok(())
    })?;
    out.push_str(&text[cursor..]);
    Ok((out, count))
}

pub struct Rules {
    pub values: Vec<Rule>,
    scopes: Vec<Option<HashSet<String>>>,
    regex: Vec<Option<PortableRegex>>,
}
pub struct View {
    pub table: String,
    exact: HashMap<String, usize>,
    subs: Vec<usize>,
    regex: Vec<usize>,
    eligible: Vec<usize>,
    excluded: Vec<(usize, &'static str)>,
}

impl Rules {
    pub fn new(values: Vec<Rule>) -> Result<Self> {
        let mut scopes = Vec::new();
        let mut regex = Vec::new();
        for (index, r) in values.iter().enumerate() {
            if r.old.is_empty()
                || r.old.len() > MAX_STRING
                || r.new.len() > MAX_STRING
                || r.old.trim_start().starts_with('#')
            {
                return Err(invalid(format!("第 {} 条词条文字无效或过长。", index + 1)));
            }
            if [&r.old, &r.new, &r.source, &r.table]
                .iter()
                .any(|s| s.chars().any(|c| (c as u32) < 32))
            {
                return Err(invalid("词条不能包含控制字符或换行。"));
            }
            let scope = if r.table.is_empty() {
                None
            } else {
                let names: Vec<String> = if r.table.starts_with('[') {
                    serde_json::from_str(&r.table)?
                } else {
                    vec![r.table.clone()]
                };
                if names.is_empty() || names.iter().any(|n| n.is_empty() || n.trim() != n) {
                    return Err(invalid("来源表列表不能为空，表名不能包含两端空格。"));
                }
                Some(names.into_iter().collect::<HashSet<String>>())
            };
            for previous in 0..index {
                let p = &values[previous];
                if p.old == r.old && p.mode == r.mode {
                    let conflict = match (&scope, &scopes[previous]) {
                        (None, None) => true,
                        (Some(a), Some(b)) => !a.is_disjoint(b),
                        _ => false,
                    };
                    if conflict {
                        return Err(invalid(format!(
                            "第 {} 条与第 {} 条词条冲突。",
                            index + 1,
                            previous + 1
                        )));
                    }
                }
            }
            regex.push(match r.mode.as_str() {
                "exact" | "sub" => None,
                "re" => Some(PortableRegex::new(&r.old, &r.new)?),
                _ => return Err(invalid("未知匹配方式。")),
            });
            scopes.push(scope);
        }
        Ok(Self {
            values,
            scopes,
            regex,
        })
    }
    pub fn validate_tables(&self, tables: &[String]) -> Result<()> {
        let mut missing: Vec<_> = self
            .scopes
            .iter()
            .flatten()
            .flatten()
            .filter(|name| !tables.contains(name))
            .cloned()
            .collect();
        missing.sort();
        missing.dedup();
        if !missing.is_empty() {
            return Err(invalid(format!(
                "来源表不存在或不在当前扫描范围内：{}。请检查表名或重新读取来源表。",
                missing.join("、")
            )));
        }
        Ok(())
    }
    pub fn view(&self, table: &str) -> View {
        let overrides: HashSet<_> = self
            .values
            .iter()
            .enumerate()
            .filter(|(i, _)| self.scopes[*i].as_ref().is_some_and(|s| s.contains(table)))
            .map(|(_, r)| (r.old.as_str(), r.mode.as_str()))
            .collect();
        let mut view = View {
            table: table.into(),
            exact: HashMap::new(),
            subs: Vec::new(),
            regex: Vec::new(),
            eligible: Vec::new(),
            excluded: Vec::new(),
        };
        for (i, r) in self.values.iter().enumerate() {
            if self.scopes[i].as_ref().is_some_and(|s| !s.contains(table)) {
                view.excluded
                    .push((i, "原词出现在未选中的来源表里。请核对来源范围。"));
                continue;
            }
            if self.scopes[i].is_none() && overrides.contains(&(r.old.as_str(), r.mode.as_str())) {
                view.excluded
                    .push((i, "该来源表有同原词的专用规则，已覆盖这条全局规则。"));
                continue;
            }
            view.eligible.push(i);
            match r.mode.as_str() {
                "exact" => {
                    view.exact.insert(r.old.clone(), i);
                }
                "sub" => view.subs.push(i),
                _ => view.regex.push(i),
            }
        }
        view.subs
            .sort_by_key(|i| std::cmp::Reverse(self.values[*i].old.chars().count()));
        view
    }
    fn matches(&self, i: usize, text: &str) -> Result<bool> {
        let r = &self.values[i];
        Ok(match r.mode.as_str() {
            "exact" => text == r.old,
            "sub" => text.contains(&r.old),
            _ => self.regex[i]
                .as_ref()
                .unwrap()
                .pattern
                .is_match(text)
                .map_err(regex_error)?,
        })
    }
}

impl View {
    fn observe(&self, rules: &Rules, text: &str, e: &mut Evidence, ascii: bool) -> Result<()> {
        if !e.explain {
            return Ok(());
        }
        for &(i, reason) in &self.excluded {
            if (!ascii || rules.values[i].mode == "exact")
                && e.misses[i].is_none()
                && rules.matches(i, text)?
            {
                e.miss(i, reason, text, &self.table);
            }
        }
        for (old, &i) in &self.exact {
            if text != old && text.contains(old) {
                e.miss(
                    i,
                    "“整段相同”要求整段文字一致；示例中原词只是句子的一部分。可改为“包含文字”。",
                    text,
                    &self.table,
                );
            }
        }
        Ok(())
    }
    pub fn skipped(
        &self,
        rules: &Rules,
        text: &str,
        e: &mut Evidence,
        targeted: bool,
    ) -> Result<()> {
        self.observe(rules, text, e, false)?;
        if e.explain {
            for &i in &self.eligible {
                if rules.matches(i, text)? {
                    e.miss(
                        i,
                        if targeted {
                            "这段文本已有单条修改，使用指定的最终文字，不再执行通用词表。"
                        } else {
                            "匹配文字所在的整段文本含日文假名，已按当前选项跳过。"
                        },
                        text,
                        &self.table,
                    );
                }
            }
        }
        Ok(())
    }
    pub fn apply(
        &self,
        rules: &Rules,
        text: &str,
        e: &mut Evidence,
        ascii: bool,
    ) -> Result<Option<String>> {
        self.observe(rules, text, e, ascii)?;
        if let Some(&i) = self.exact.get(text) {
            let new = &rules.values[i].new;
            if !ascii && e.explain {
                for &other in &self.eligible {
                    if other != i && rules.matches(other, text)? {
                        e.miss(
                            other,
                            "整段相同规则已占用这段原文，其他重叠规则不再处理。",
                            text,
                            &self.table,
                        );
                    }
                }
            }
            if new != text {
                e.hits[i] += 1;
                return Ok(Some(new.clone()));
            }
            e.miss(i, "原文字与替换文字相同，无需修改。", text, &self.table);
            return Ok(None);
        }
        if ascii {
            return Ok(None);
        }
        let mut occupied = vec![false; text.len() + 1];
        let mut zeroes = HashSet::new();
        let mut edits = Vec::new();
        let mut result_size = text.len();
        let mut accept = |start: usize, end: usize, replacement: String, i: usize| -> Result<()> {
            if (start == end && (zeroes.contains(&start) || occupied[start]))
                || (start != end
                    && (occupied[start..end].iter().any(|v| *v)
                        || zeroes.iter().any(|p| start <= *p && *p < end)))
            {
                e.miss(
                    i,
                    "匹配位置与更高优先级的原文匹配重叠，已保留优先规则。",
                    text,
                    &self.table,
                );
                return Ok(());
            }
            result_size = result_size - (end - start) + replacement.len();
            if result_size > MAX_STRING {
                return Err(invalid("替换结果过长，请缩短替换文字。"));
            }
            if start == end {
                zeroes.insert(start);
            } else {
                occupied[start..end].fill(true);
            }
            edits.push((start, end, replacement));
            e.hits[i] += 1;
            Ok(())
        };
        for &i in &self.subs {
            let r = &rules.values[i];
            for (at, _) in text.match_indices(&r.old) {
                accept(at, at + r.old.len(), r.new.clone(), i)?;
            }
        }
        for &i in &self.regex {
            let re = rules.regex[i].as_ref().unwrap();
            re.for_each(text, |caps| {
                let m = caps.get(0).unwrap();
                accept(m.start(), m.end(), re.expand(caps)?, i)?;
                Ok(())
            })?;
        }
        if edits.is_empty() {
            return Ok(None);
        }
        edits.sort_by_key(|(a, b, _)| (*a, *b));
        let mut out = String::with_capacity(text.len());
        let mut cursor = 0;
        for (start, end, value) in edits {
            out.push_str(&text[cursor..start]);
            out.push_str(&value);
            cursor = end;
        }
        out.push_str(&text[cursor..]);
        Ok(if out == text { None } else { Some(out) })
    }
}
