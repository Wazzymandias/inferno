//! Backend cache event subscriptions and recovery.

mod vllm;

pub(crate) use vllm::Subscription;

fn invalid(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}
