use nom::{
    IResult,
    sequence::{delimited, tuple},
    character::complete::char,
    bytes::complete::{is_not, tag},
    branch::alt,
    multi::many0,
    combinator::opt,
};
use vertex::requests::{ReportStatus, SearchCriteria};
use chrono::{DateTime, Utc, TimeZone, NaiveDate};
use nom::error::ErrorKind;
use itertools::Itertools;

struct LabelledTerm<'a> {
    name: &'a str,
    text: &'a str,
}

#[derive(Debug)]
pub enum Criterion {
    OfUser(String),
    ByUser(String),
    BeforeDate(DateTime<Utc>),
    AfterDate(DateTime<Utc>),
    InCommunity(String),
    InRoom(String),
    Status(ReportStatus),
}

enum Term<'a> {
    Criterion(Criterion),
    Word(&'a str),
}

fn quotes(input: &str) -> IResult<&str, &str> {
    delimited(char('"'), is_not("\""), char('"'))(input)
}

fn labelled_term_text(input: &str) -> IResult<&str, &str> {
    alt((quotes, is_not(" ")))(input)
}

fn labelled_term(input: &str) -> IResult<&str, LabelledTerm<'_>> {
    tuple((is_not(": "), tag(":"), labelled_term_text, opt(tag(" "))))(input)
        .map(|(a, (name, _, text, _))| {
            (a, LabelledTerm { name, text})
        })
}

fn parse_date(input: &str) -> IResult<&str, DateTime<Utc>> {
    NaiveDate::parse_from_str(input, "%F")
        .map(|d| (input, Utc.from_utc_date(&d).and_hms(0, 0, 0)))
        .map_err(|_| nom::Err::Error((input, ErrorKind::ParseTo)))
}

fn criterion_from_term<'a>((txt, term): (&'a str, LabelledTerm<'a>)) -> IResult<&'a str, Criterion> {
    let criterion = match term.name {
        "of" => Criterion::OfUser(term.text.to_string()),
        "by" => Criterion::ByUser(term.text.to_string()),
        "before" => Criterion::BeforeDate(parse_date(term.text)?.1),
        "after" => Criterion::AfterDate(parse_date(term.text)?.1),
        "community" => Criterion::InCommunity(term.text.to_string()),
        "room" => Criterion::InRoom(term.text.to_string()),
        "is" => {
            let status = match term.text.to_lowercase().as_str() {
                "open" | "opened" => ReportStatus::Opened,
                "accepted" | "accept" | "agreed" | "agree" => ReportStatus::Accepted,
                "denied" | "deny" | "rejected" | "reject" => ReportStatus::Denied,
                _ => return Err(nom::Err::Error((txt, ErrorKind::ParseTo)))
            };

            Criterion::Status(status)
        }
        _ => return Err(nom::Err::Error((txt, ErrorKind::ParseTo)))
    };

    Ok((txt, criterion))
}

fn word(input: &str) -> IResult<&str, &str> {
    tuple((is_not(" "), opt(tag(" "))))(input)
        .map(|(a, (b, _))| (a, b))
}

pub fn do_parse(input: &str) -> IResult<&str, SearchCriteria> {
    let branches = alt((
        |x| {
            labelled_term(x)
                .and_then(criterion_from_term)
                .map(|(a, b)| (a, Term::Criterion(b)))
        },
        |x| word(x).map(|(a, b)| (a, Term::Word(b)))
    ));
    let (criteria, words): (Vec<Term>, Vec<Term>) = many0(branches)(input)?
        .1
        .into_iter()
        .partition(|x| matches!(x, Term::Criterion(_)));
    
    let mut criteria: Vec<Criterion> = criteria.into_iter()
        .filter_map(|x| match x {
            Term::Criterion(c) => Some(c),
            _ => None,
        })
        .collect();
    criteria.dedup_by_key(|x| std::mem::discriminant(x));
    
    let words = words.into_iter()
        .filter_map(|x| match x {
            Term::Word(w) => Some(w.to_string()),
            _ => None,
        })
        .join(" ");

    let mut search_criteria = SearchCriteria { words, ..Default::default() };

    for criterion in criteria {
        match criterion {
            Criterion::OfUser(name) => search_criteria.of_user = Some(name),
            Criterion::ByUser(name) => search_criteria.by_user = Some(name),
            Criterion::BeforeDate(date) => search_criteria.before_date = Some(date),
            Criterion::AfterDate(date) => search_criteria.after_date = Some(date),
            Criterion::InCommunity(name) => search_criteria.in_community = Some(name),
            Criterion::InRoom(name) => search_criteria.in_room = Some(name),
            Criterion::Status(status) => search_criteria.status = Some(status),
        }
    }
    
    Ok((input, search_criteria))
}
