pub mod bundles {
    use fluent_bundle::{concurrent::FluentBundle, FluentResource};
    use std::sync::LazyLock;
    use std::sync::OnceLock;
    use unic_langid::langid;

    type Bundle = FluentBundle<FluentResource>;

    macro_rules! bundle {
        ($var:ident = $id:literal) => {
            pub static $var: LazyLock<Bundle> = LazyLock::new(|| {
                let mut bundle = Bundle::new_concurrent(vec![langid!($id)]);
                let resource = FluentResource::try_new(
                    include_str!(concat!("../localization/", $id, "/messages.ftl")).into(),
                )
                .expect(concat!("Failed to init ", $id, " locale"));
                bundle.add_resource(resource).expect(concat!(
                    "Failed to add ",
                    $id,
                    " locale resource"
                ));
                bundle
            });
        };
    }

    bundle!(EN_US = "en-US");
    bundle!(TOK = "tok");

    pub fn locale() -> &'static [&'static LazyLock<Bundle>] {
        LOCALE.get_or_init(init_from_env)
    }

    pub fn init_from_env() -> &'static [&'static LazyLock<Bundle>] {
        let lang = std::env::var("LC_MESSAGES")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default();
        init_to(&lang)
    }

    pub fn init_to(s: &str) -> &'static [&'static LazyLock<Bundle>] {
        match s {
            "en-US" => const { &[&EN_US] },
            "tok" => const { &[&TOK, &EN_US] },
            _ => const { &[&EN_US] },
        }
    }

    pub static LOCALE: OnceLock<&[&LazyLock<Bundle>]> = OnceLock::new();
}

#[macro_export]
macro_rules! ftl_args {
    ($msg_id:literal) => {
        $crate::ftl_args!($msg_id,)
    };
    ($msg_id:literal, $($var_name:ident = $var_val:expr),*) => {
        {
            $crate::localization::bundles::locale()
                .into_iter()
                .filter_map(|l| Some((l, l.get_message($msg_id)?.value()?)))
                .next()
                .map(|(bundle, pattern)| {
                    #[allow(unused_mut)]
                    let mut args = fluent_bundle::FluentArgs::new();
                    $(
                        args.set(stringify!($var_name), $var_val);
                    )*
                    (bundle, pattern, args)
                })
        }
    };
}

#[macro_export]
macro_rules! ftl_format {
    ($msg_id:literal) => {
        $crate::ftl_format!($msg_id,)
    };
    ($msg_id:literal, $($var_name:ident = $var_val:expr),*) => {
        {
            let args = $crate::ftl_args!($msg_id, $($var_name = $var_val),*);
            if let Some((bundle, pattern, args)) = args {
                let mut errors = Vec::new();
                let out = bundle.format_pattern(pattern, Some(&args), &mut errors);
                if !errors.is_empty() {
                    for error in errors {
                        eprintln!("{error}");
                    }
                    panic!("failed to format localized text");
                }
                out
            } else {
                std::borrow::Cow::from($msg_id)
            }

        }
    }
}

#[macro_export]
macro_rules! ftl_write {
    ($writer:expr, $msg_id:literal) => {
        $crate::ftl_write!($writer, $msg_id,)
    };
    ($writer:expr, $msg_id:literal, $($var_name:ident = $var_val:expr),*) => {
        {
            use std::fmt::Write;
            let args = $crate::ftl_args!($msg_id, $($var_name = $var_val),*);
            if let Some((bundle, pattern, args)) = args {
                let mut errors = Vec::new();
                bundle.write_pattern($writer, pattern, Some(&args), &mut errors).expect("failed to write localized text");
                if !errors.is_empty() {
                    for error in errors {
                        eprintln!("{error}");
                    }
                    panic!("failed to format localized text");
                }
            } else {
                write!($writer, "{}", $msg_id).expect("failed to write text message id");
            }
        }
    }
}

pub struct WriterCompat<W>(pub W);

impl<W: std::io::Write> std::fmt::Write for WriterCompat<W> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write_all(s.as_bytes()).map_err(|_| std::fmt::Error)
    }
}

#[macro_export]
macro_rules! ftl_print {
    ($msg_id:literal) => {
        $crate::ftl_print!($msg_id,)
    };
    ($msg_id:literal, $($var_name:ident = $var_val:expr),*) => {
        {
            let mut stdout = $crate::localization::WriterCompat(std::io::stdout());
            $crate::ftl_write!(&mut stdout, $msg_id, $($var_name = $var_val),*);
        }
    }
}

#[macro_export]
macro_rules! ftl_println {
    ($msg_id:literal) => {
        $crate::ftl_println!($msg_id,)
    };
    ($msg_id:literal, $($var_name:ident = $var_val:expr),*) => {
        {
            use std::fmt::Write;
            let mut stdout = $crate::localization::WriterCompat(std::io::stdout());
            $crate::ftl_write!(&mut stdout, $msg_id, $($var_name = $var_val),*);
            writeln!(&mut stdout).expect("failed to write newline");
        }
    }
}

#[macro_export]
macro_rules! ftl_eprint {
    ($msg_id:literal) => {
        $crate::ftl_eprint!($msg_id,)
    };
    ($msg_id:literal, $($var_name:ident = $var_val:expr),*) => {
        {
            let mut stderr = $crate::localization::WriterCompat(std::io::stderr());
            $crate::ftl_write!(&mut stderr, $msg_id, $($var_name = $var_val),*);
        }
    }
}

#[macro_export]
macro_rules! ftl_eprintln {
    ($msg_id:literal) => {
        $crate::ftl_eprintln!($msg_id,)
    };
    ($msg_id:literal, $($var_name:ident = $var_val:expr),*) => {
        {
            use std::fmt::Write;
            let mut stderr = $crate::localization::WriterCompat(std::io::stderr());
            $crate::ftl_write!(&mut stderr, $msg_id, $($var_name = $var_val),*);
            writeln!(&mut stderr).expect("failed to write newline");
        }
    }
}
