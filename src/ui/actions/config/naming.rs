use r_wg::application::ConfigLibraryService;

pub(crate) fn next_available_name<'a>(
    names: impl IntoIterator<Item = &'a str>,
    base: &str,
) -> String {
    ConfigLibraryService::new().next_available_name(names, base)
}

#[cfg(test)]
mod tests {
    use super::next_available_name;
    use r_wg::application::ConfigLibraryService;
    use std::collections::HashSet;

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
        let service = ConfigLibraryService::new();
        let mut names = HashSet::from(["alpha".to_string()]);
        let reserved = service.reserve_unique_name(&mut names, "alpha");
        assert_eq!(reserved, "alpha-2");
        assert!(names.contains("alpha-2"));
    }
}
