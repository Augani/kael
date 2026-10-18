macro_rules! trace {
    ($($tt:tt)*) => {{
        #[cfg(feature = "tracing")]
        {
            tracing::trace!($($tt)*);
        }

        #[cfg(not(feature = "tracing"))]
        {
        }
    }};
}

macro_rules! span_enter {
    ($level:expr, $name:literal $(, $($fields:tt)+)? ) => {
        #[cfg(feature = "tracing")]
        let _span = tracing::span!($level, $name $(, $($fields)+)?).entered();

        #[cfg(not(feature = "tracing"))]
        let _span = ();
    };
}
