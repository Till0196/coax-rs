// Copyright 2026 Till0196
// SPDX-License-Identifier: Apache-2.0

//! 小さな XML ツリー。放送局が送る TTML（[`crate::ttml`]）を読む範囲だけ：
//! 要素と属性とテキスト、実体参照、CDATA、コメントと処理命令の読み
//! 飛ばし。名前空間の接頭辞（`tts:`）は名前から除いて持つ（放送局のドキュメントに
//! 同じローカル名の衝突は無い）。壊れていても読めたところまで。

/// 要素。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Element {
    /// ローカル名（`tts:color` なら `color`）。
    pub name: String,
    /// 属性（ローカル名、値）。
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    /// 属性（ローカル名で）。
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// 子の要素。
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// 子孫の要素をドキュメントの順で。
    pub fn descendants(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        fn walk<'a>(e: &'a Element, out: &mut Vec<&'a Element>) {
            for child in e.elements() {
                out.push(child);
                walk(child, out);
            }
        }
        walk(self, &mut out);
        out
    }

    /// 名前の子孫をドキュメントの順で。
    pub fn find_all(&self, name: &str) -> Vec<&Element> {
        self.descendants()
            .into_iter()
            .filter(|e| e.name == name)
            .collect()
    }
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// 実体参照を戻す。
pub fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let body = &rest[1..end];
        let replaced = if let Some(number) = body.strip_prefix('#') {
            let code = if let Some(hex) = number.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                number.parse::<u32>().ok()
            };
            code.and_then(char::from_u32).map(|c| c.to_string())
        } else {
            match body {
                "lt" => Some("<".into()),
                "gt" => Some(">".into()),
                "amp" => Some("&".into()),
                "quot" => Some("\"".into()),
                "apos" => Some("'".into()),
                _ => None,
            }
        };
        match replaced {
            Some(s) => {
                out.push_str(&s);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// 引用符の中の `>` を読み飛ばして、タグの終わりを見つける。
fn tag_end(text: &[char], from: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, &c) in text.iter().enumerate().skip(from + 1) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return Some(i),
            None => {}
        }
    }
    None
}

/// タグの内容（名前と属性）。
fn element(inner: &str) -> Element {
    let inner = inner.trim_start();
    let name_end = inner
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(inner.len());
    let name = local(&inner[..name_end]).to_owned();
    let mut attrs = Vec::new();
    let rest: Vec<char> = inner[name_end..].chars().collect();
    let mut i = 0;
    while i < rest.len() {
        while i < rest.len() && (rest[i].is_whitespace() || rest[i] == '/') {
            i += 1;
        }
        if i >= rest.len() {
            break;
        }
        let start = i;
        while i < rest.len() && !rest[i].is_whitespace() && rest[i] != '=' && rest[i] != '/' {
            i += 1;
        }
        let key: String = rest[start..i].iter().collect();
        if key.is_empty() {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < rest.len() && rest[j].is_whitespace() {
            j += 1;
        }
        let mut value = String::new();
        if j < rest.len() && rest[j] == '=' {
            j += 1;
            while j < rest.len() && rest[j].is_whitespace() {
                j += 1;
            }
            if j < rest.len() && (rest[j] == '"' || rest[j] == '\'') {
                let q = rest[j];
                j += 1;
                let vs = j;
                while j < rest.len() && rest[j] != q {
                    j += 1;
                }
                value = rest[vs..j].iter().collect();
                j += 1;
            } else {
                let vs = j;
                while j < rest.len() && !rest[j].is_whitespace() {
                    j += 1;
                }
                value = rest[vs..j].iter().collect();
            }
            i = j;
        }
        attrs.push((local(&key).to_owned(), decode(&value)));
    }
    Element {
        name,
        attrs,
        children: Vec::new(),
    }
}

/// 要素の入れ子の上限。放送局のドキュメントは十階層もない。
const MOST_DEPTH: usize = 64;

/// ドキュメントを読む。ルート要素（無ければ None）。
pub fn parse(text: &str) -> Option<Element> {
    let chars: Vec<char> = text.chars().collect();
    let mut stack: Vec<Element> = vec![Element::default()];
    let push_child = |stack: &mut Vec<Element>, child: Node| {
        if let Some(parent) = stack.last_mut() {
            parent.children.push(child);
        }
    };
    let find = |from: usize, pattern: &str| -> Option<usize> {
        let p: Vec<char> = pattern.chars().collect();
        if chars.len() < p.len() {
            return None;
        }
        (from..=chars.len() - p.len()).find(|&i| chars[i..i + p.len()] == p[..])
    };
    let starts = |at: usize, pattern: &str| -> bool {
        let p: Vec<char> = pattern.chars().collect();
        chars.len() >= at + p.len() && chars[at..at + p.len()] == p[..]
    };
    let slice = |a: usize, b: usize| -> String { chars[a..b.min(chars.len())].iter().collect() };
    let mut at = 0;
    let length = chars.len();
    while at < length {
        let Some(lt) = find(at, "<") else {
            push_child(&mut stack, Node::Text(decode(&slice(at, length))));
            break;
        };
        if lt > at {
            push_child(&mut stack, Node::Text(decode(&slice(at, lt))));
        }
        if starts(lt, "<!--") {
            at = find(lt, "-->").map_or(length, |e| e + 3);
            continue;
        }
        if starts(lt, "<![CDATA[") {
            let end = find(lt, "]]>");
            push_child(&mut stack, Node::Text(slice(lt + 9, end.unwrap_or(length))));
            at = end.map_or(length, |e| e + 3);
            continue;
        }
        if starts(lt, "<?") || starts(lt, "<!") {
            at = find(lt, ">").map_or(length, |e| e + 1);
            continue;
        }
        let Some(gt) = tag_end(&chars, lt) else {
            break;
        };
        let inner = slice(lt + 1, gt);
        at = gt + 1;
        if let Some(closing) = inner.strip_prefix('/') {
            let closing = local(closing.trim_end_matches([' ', '/', '\t', '\n', '\r']));
            if let Some(depth) = stack.iter().rposition(|e| e.name == closing)
                && depth > 0
            {
                while stack.len() > depth {
                    let done = stack.pop().expect("あった");
                    push_child(&mut stack, Node::Element(done));
                }
            }
            continue;
        }
        let empty = inner.ends_with('/');
        let inner = if empty {
            &inner[..inner.len() - 1]
        } else {
            &inner
        };
        let el = element(inner);
        // 入れ子は上限まで。それより深い要素は内容の無い要素として扱い、
        // 内容は現在の親に続ける（ツリーを辿る側は再帰なので、際限の無い入れ子は
        // スタックを使い切る）。
        if empty || stack.len() > MOST_DEPTH {
            push_child(&mut stack, Node::Element(el));
        } else {
            stack.push(el);
        }
    }
    while stack.len() > 1 {
        let done = stack.pop().expect("あった");
        push_child(&mut stack, Node::Element(done));
    }
    let root = stack.pop()?;
    root.children.into_iter().find_map(|n| match n {
        Node::Element(e) => Some(e),
        Node::Text(_) => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_attributes_and_text_are_read_without_prefixes() {
        let root = parse(
            "<?xml version=\"1.0\"?><tt xmlns:tts=\"x\"><head><style xml:id=\"s\" tts:color=\"#fff\"/></head><body><p>a&amp;b<br/>c<![CDATA[<d>]]></p><!-- x --></body></tt>",
        )
        .expect("読める");
        assert_eq!(root.name, "tt");
        let style = root.find_all("style")[0];
        assert_eq!(style.attr("id"), Some("s"));
        assert_eq!(style.attr("color"), Some("#fff"));
        let p = root.find_all("p")[0];
        let texts: Vec<&str> = p
            .children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["a&b", "c", "<d>"]);
        assert_eq!(p.elements().count(), 1);
    }

    #[test]
    fn nesting_without_end_stops_at_the_limit() {
        let text = "<a>".repeat(100_000);
        let root = parse(&text).expect("読める");
        let mut depth = 0;
        let mut at = &root;
        while let Some(child) = at.elements().next() {
            depth += 1;
            at = child;
        }
        assert!(depth <= MOST_DEPTH + 1, "{depth}");
    }
}
