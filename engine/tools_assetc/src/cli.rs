//! The command line: `assetc [--srgb] [--no-mips] <input> <output>`.

use crate::ConvertError;
use crate::convert::{Options, convert};
use core::fmt;
use std::fs;

/// Usage text printed on a wrong command line.
pub const USAGE: &str = "usage: assetc [--srgb] [--no-mips] <input.ppm|input.pam> <output.dtex>";

/// Why a run failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliError {
    /// The arguments were not understood.
    Usage,
    /// The input file could not be read.
    Read,
    /// The output file could not be written.
    Write,
    /// The conversion failed.
    Convert(ConvertError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => f.write_str(USAGE),
            Self::Read => f.write_str("cannot read the input file"),
            Self::Write => f.write_str("cannot write the output file"),
            Self::Convert(e) => write!(f, "conversion failed: {e}"),
        }
    }
}

impl std::error::Error for CliError {}

/// Parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// Source image path.
    pub input: String,
    /// Destination path.
    pub output: String,
    /// Conversion options.
    pub options: Options,
}

/// Parses the arguments after the program name.
///
/// # Errors
/// [`CliError::Usage`] for an unknown flag or a count of paths other than two.
pub fn parse_args(args: &[String]) -> Result<Args, CliError> {
    let mut options = Options {
        srgb: false,
        mips: true,
    };
    let mut paths = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--srgb" => options.srgb = true,
            "--no-mips" => options.mips = false,
            flag if flag.starts_with("--") => return Err(CliError::Usage),
            path => paths.push(path.to_string()),
        }
    }
    match <[String; 2]>::try_from(paths) {
        Ok([input, output]) => Ok(Args {
            input,
            output,
            options,
        }),
        Err(_) => Err(CliError::Usage),
    }
}

/// Runs one conversion. Nothing is written when the conversion fails.
///
/// # Errors
/// See [`CliError`].
pub fn run(args: &[String]) -> Result<(), CliError> {
    let args = parse_args(args)?;
    let source = fs::read(&args.input).map_err(|_| CliError::Read)?;
    let converted = convert(&source, args.options).map_err(CliError::Convert)?;
    fs::write(&args.output, converted).map_err(|_| CliError::Write)
}

#[cfg(test)]
mod tests;
