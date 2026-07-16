#![no_main]

use fhp_tokenizer::streaming::StreamTokenizer;
use fhp_tokenizer::token::Token;
use libfuzzer_sys::fuzz_target;

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Open(String, Vec<(String, Option<String>)>, bool),
    Close(String),
    Text(String),
    Comment(String),
    Doctype(String),
    Cdata(String),
}

fn normalize<'a>(tokens: impl IntoIterator<Item = Token<'a>>) -> Vec<Event> {
    let mut events = Vec::new();
    for token in tokens {
        let event = match token {
            Token::OpenTag {
                name,
                attributes,
                self_closing,
                ..
            } => Event::Open(
                name.into_owned(),
                attributes
                    .into_iter()
                    .map(|attribute| {
                        (
                            attribute.name.into_owned(),
                            attribute.value.map(|value| value.into_owned()),
                        )
                    })
                    .collect(),
                self_closing,
            ),
            Token::CloseTag { name, .. } => Event::Close(name.into_owned()),
            Token::Text { content } => {
                if let Some(Event::Text(previous)) = events.last_mut() {
                    previous.push_str(&content);
                    continue;
                }
                Event::Text(content.into_owned())
            }
            Token::Comment { content } => Event::Comment(content.into_owned()),
            Token::Doctype { content } => Event::Doctype(content.into_owned()),
            Token::CData { content } => Event::Cdata(content.into_owned()),
        };
        events.push(event);
    }
    events
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let chunk_size = usize::from(data[0]).max(1);
    let html = String::from_utf8_lossy(&data[1..]);
    let expected = normalize(fhp_tokenizer::tokenize(&html));

    let mut tokenizer = StreamTokenizer::new();
    let mut streamed = Vec::new();
    for chunk in html.as_bytes().chunks(chunk_size) {
        streamed.extend(tokenizer.feed(chunk));
    }
    streamed.extend(tokenizer.finish());

    assert_eq!(normalize(streamed), expected);
});
