use once_cell::sync::Lazy;

static ASCII_LOGO: Lazy<Vec<String>> = Lazy::new(|| {
    vec![
        r"                          $$\           ".to_string(),
        r"                          $$ |          ".to_string(),
        r" $$$$$$$\  $$$$$$\   $$$$$$$ | $$$$$$\  ".to_string(),
        r"$$  _____|$$  __$$\ $$  __$$ |$$  __$$\ ".to_string(),
        r"$$ /      $$ /  $$ |$$ /  $$ |$$ |  \__|".to_string(),
        r"$$ |      $$ |  $$ |$$ |  $$ |$$ |      ".to_string(),
        r"\$$$$$$$\ \$$$$$$  |\$$$$$$$ |$$ |      ".to_string(),
        r" \_______| \______/  \_______|\__|      ".to_string(),
    ]
});

pub fn get_logo() -> Option<&'static Vec<String>> {
    Some(&ASCII_LOGO)
}

pub fn get_logo_dimensions() -> Option<(usize, usize)> {
    let width = ASCII_LOGO.first().map(|l| l.len()).unwrap_or(0);
    Some((width, ASCII_LOGO.len()))
}
