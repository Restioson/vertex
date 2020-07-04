#[macro_export]
macro_rules! extract_path {
    (
        @parsed [$($parsed:tt)*]
        $ident:ident
        $($rest:tt)*
    ) => (extract_path! {
        @parsed [$($parsed)* $ident]
        $($rest)*
    });

    (
        @parsed [$($parsed:tt)*]
        ::
        $($rest:tt)*
    ) => (extract_path! {
        @parsed [$($parsed)* ::]
        $($rest)*
    });

    (
        @parsed [$($parsed:tt)*]
        $($otherwise:tt)*
    ) => (
        stringify!($($parsed)*)
    );

    (
        $($path:tt)*
    ) => (extract_path! {
        @parsed []
        $($path)*
    });
}

#[macro_export]
macro_rules! expect {
    (if let $pat:pat = $v:ident { $expr: expr }) => {
        defile::expr!{
            match $v {
                $pat => $expr,
                other => Err(Error::UnexpectedMessage {
                    expected: extract_path!(@$pat),
                    got: Box::new(other),
                }),
            }
        }
    };
}
