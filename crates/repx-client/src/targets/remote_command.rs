use super::common::shell_quote;

#[derive(Debug, Clone)]
pub struct RemoteCommand {
    parts: Vec<String>,
}

impl RemoteCommand {
    pub fn new(program: &str) -> Self {
        Self {
            parts: vec![program.to_string()],
        }
    }

    pub fn arg(mut self, arg: &str) -> Self {
        self.parts.push(shell_quote(arg));
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for arg in args {
            self.parts.push(shell_quote(arg.as_ref()));
        }
        self
    }

    fn append_raw(mut self, s: &str) -> Self {
        self.parts.push(s.to_string());
        self
    }

    pub fn and(self, other: RemoteCommand) -> Self {
        self.append_raw("&&").merge(other)
    }

    pub fn or(self, other: RemoteCommand) -> Self {
        self.append_raw("||").merge(other)
    }

    pub fn pipe(self, other: RemoteCommand) -> Self {
        self.append_raw("|").merge(other)
    }

    pub fn redirect_out(self, path: &str) -> Self {
        self.append_raw(">").append_raw(&shell_quote(path))
    }

    fn merge(mut self, other: RemoteCommand) -> Self {
        self.parts.extend(other.parts);
        self
    }

    pub fn to_shell_string(&self) -> String {
        self.parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_command() {
        let cmd = RemoteCommand::new("ls").arg("-l").arg("/tmp");
        assert_eq!(cmd.to_shell_string(), "ls '-l' '/tmp'");
    }

    #[test]
    fn test_quoting() {
        let cmd = RemoteCommand::new("echo").arg("hello world").arg("it's me");
        assert_eq!(cmd.to_shell_string(), "echo 'hello world' 'it'\\''s me'");
    }

    #[test]
    fn test_chaining() {
        let cmd1 = RemoteCommand::new("mkdir").arg("-p").arg("foo");
        let cmd2 = RemoteCommand::new("cd").arg("foo");
        let combined = cmd1.and(cmd2);
        assert_eq!(combined.to_shell_string(), "mkdir '-p' 'foo' && cd 'foo'");
    }

    #[test]
    fn test_piping() {
        let cmd1 = RemoteCommand::new("cat").arg("file.txt");
        let cmd2 = RemoteCommand::new("grep").arg("pattern");
        let combined = cmd1.pipe(cmd2);
        assert_eq!(
            combined.to_shell_string(),
            "cat 'file.txt' | grep 'pattern'"
        );
    }

    #[test]
    fn test_redirect() {
        let cmd = RemoteCommand::new("echo")
            .arg("hello")
            .redirect_out("file.txt");
        assert_eq!(cmd.to_shell_string(), "echo 'hello' > 'file.txt'");
    }
}
