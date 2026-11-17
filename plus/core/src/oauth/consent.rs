use std::cell::RefCell;

type Observer = Box<dyn Fn(&str)>;

thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
}

struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        OBSERVER.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Runs `run` with `observer` receiving the consent URL of any sign-in that `run` starts on this
/// thread, so the sign-in does not open the OS browser. Upstream's interactive flow runs on the
/// calling thread, which is why the observer is scoped to it.
pub fn with_observer<T>(observer: impl Fn(&str) + 'static, run: impl FnOnce() -> T) -> T {
    OBSERVER.with(|slot| *slot.borrow_mut() = Some(Box::new(observer)));
    let _reset = Reset;
    run()
}

/// Hands `url` to the observer of this thread. False when there is none: the caller opens the
/// browser itself.
pub fn present(url: &str) -> bool {
    OBSERVER.with(|slot| match slot.borrow().as_ref() {
        Some(observer) => {
            observer(url);
            true
        }
        None => false,
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{present, with_observer};

    #[test]
    fn without_an_observer_the_caller_keeps_the_browser() {
        assert!(!present("https://as.example.invalid/authorize"));
    }

    #[test]
    fn the_observer_gets_the_url_and_takes_the_browser_over() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        let taken = with_observer(
            move |url| sink.borrow_mut().push(url.to_string()),
            || present("https://as.example.invalid/authorize?state=s"),
        );
        assert!(taken);
        assert_eq!(
            *seen.borrow(),
            ["https://as.example.invalid/authorize?state=s"]
        );
    }

    #[test]
    fn the_observer_is_gone_once_the_run_ends() {
        with_observer(|_| {}, || ());
        assert!(!present("https://as.example.invalid/authorize"));
    }

    #[test]
    fn the_observer_is_gone_after_a_panic_in_the_run() {
        let result = std::panic::catch_unwind(|| with_observer(|_| {}, || panic!("flow failed")));
        assert!(result.is_err());
        assert!(!present("https://as.example.invalid/authorize"));
    }

    #[test]
    fn another_thread_keeps_the_browser() {
        let taken_elsewhere = with_observer(
            |_| {},
            || {
                std::thread::spawn(|| present("https://as.example.invalid/authorize"))
                    .join()
                    .unwrap()
            },
        );
        assert!(!taken_elsewhere);
    }
}
