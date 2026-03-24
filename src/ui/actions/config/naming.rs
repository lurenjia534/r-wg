use std::collections::HashSet;

pub(crate) fn next_available_name<'a>(
    names: impl IntoIterator<Item = &'a str>,
    base: &str,
) -> String {
    let names: HashSet<&str> = names.into_iter().collect();

    if !names.contains(base) {
        return base.to_string();
    }
    for idx in 2..1000 {
        let candidate = format!("{base}-{idx}");
        if !names.contains(candidate.as_str()) {
            return candidate;
        }
    }
    format!("{base}-{}", names.len() + 1)
}

pub(crate) fn reserve_unique_name(names_in_use: &mut HashSet<String>, base: &str) -> String {
    let candidate = next_available_name(names_in_use.iter().map(String::as_str), base);
    names_in_use.insert(candidate.clone());
    candidate
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{next_available_name, reserve_unique_name};

    #[test]
    fn next_available_name_preserves_base_when_unused() {
        let names = ["alpha", "beta"];
        assert_eq!(next_available_name(names, "gamma"), "gamma");
    }

    #[test]
    fn next_available_name_finds_first_gap() {
        let names = ["alpha", "alpha-2", "alpha-4"];
        assert_eq!(next_available_name(names, "alpha"), "alpha-3");
    }

    #[test]
    fn reserve_unique_name_updates_set() {
        let mut names = HashSet::from(["alpha".to_string()]);
        let reserved = reserve_unique_name(&mut names, "alpha");
        assert_eq!(reserved, "alpha-2");
        assert!(names.contains("alpha-2"));
    }
}
