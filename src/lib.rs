//! A crate for inspecting and extracting data from Office VBA projects.
//!
//! This is an implementation of the [\[MS-OVBA\]: Office VBA File Format Structure][MS-OVBA] protocol
//! (Revision 9.1, published 2020-02-19).
//!
//! [MS-OVBA]: https://docs.microsoft.com/en-us/openspecs/office_file_formats/ms-ovba/575462ba-bf67-4190-9fac-c275523c75fc

#![forbid(unsafe_code)]
#![warn(rust_2018_idioms, missing_docs)]

mod error;
pub use crate::error::{Error, Result};

mod parser;

use cfb::CompoundFile;

use std::io::{Cursor, Read};

/// Represents a VBA project.
///
/// This type serves as the entry point into this crate's functionality, and
/// exposes the public API surface.
pub struct Project {
    // TODO: Figure out how to make this generic (attempts have failed with
    //       trait bound violations). This would allow [`open_project`] to
    //       accept a wider range of input types.
    #[doc(hidden)]
    container: CompoundFile<Cursor<Vec<u8>>>,
}

/// Specifies the platform for which the VBA project is created.
#[derive(Debug)]
pub enum SysKind {
    /// For 16-bit Windows Platforms.
    Win16,
    /// For 32-bit Windows Platforms.
    Win32,
    /// For Macintosh Platforms.
    MacOs,
    /// For 64-bit Windows Platforms.
    Win64,
}

/// Specifies information for the VBA project, including project information,
/// project references, and modules.
#[derive(Debug)]
pub struct ProjectInformation {
    /// Specifies version-independent information for the VBA project.
    pub information: Information,
    /// Specifies the external references of the VBA project.
    pub references: Vec<Reference>,
    /// Specifies data for the modules in the project.
    pub modules: Modules,
}

/// Specifies a reference to a twiddled type library and its extended type library.
#[derive(Debug)]
pub struct ReferenceControl {
    /// (Optional) Name and NameUnicode entries
    name: Option<(String, String)>,
    libid_original: Option<String>,
    libid_twiddled: String,
    name_extended: Option<(String, String)>,
    libid_extended: String,
    guid: Vec<u8>, // Should be an `[u8; 16]`, though I'm not sure how to convert &[u8] returned by the parser into an array.
    /// Unique for each `ReferenceControl`
    cookie: u32,
}

/// Specifies the identifier of the Automation type library the containing
/// [`ReferenceControl`]'s twiddled type library was generated from.
#[derive(Debug)]
pub struct ReferenceOriginal {
    /// (Optional) Name and NameUnicode entries
    name: Option<(String, String)>,
    libid_original: String,
}

/// Specifies a reference to an Automation type library.
#[derive(Debug)]
pub struct ReferenceRegistered {
    name: Option<(String, String)>,
    libid: String,
}

/// Specifies a reference to an external VBA project.
#[derive(Debug)]
pub struct ReferenceProject {
    name: Option<(String, String)>,
    libid_absolute: String,
    libid_relative: String,
    major_version: u32,
    minor_version: u16,
}

/// Specifies a reference to an Automation type library or VBA project.
#[derive(Debug)]
pub enum Reference {
    /// The `Reference` is a [`ReferenceControl`].
    Control(ReferenceControl),
    /// The `Reference` is a [`ReferenceOriginal`].
    Original(ReferenceOriginal),
    /// The `Reference` is a [`ReferenceRegistered`].
    Registered(ReferenceRegistered),
    /// The `Reference` is a [`ReferenceProject`].
    Project(ReferenceProject),
}

/// Specifies version-independent information for the VBA project.
#[derive(Debug)]
pub struct Information {
    /// Specifies the platform for which the VBA project is created.
    pub sys_kind: SysKind,
    lcid: u32,
    lcid_invoke: u32,
    code_page: u16,
    name: String,
    doc_string: String,
    doc_string_unicode: String,
    help_file_1: String,
    help_file_2: String,
    help_context: u32,
    lib_flags: u32,
    version_major: u32,
    version_minor: u16,
    constants: String,
    constants_unicode: String,
}

/// Specifies data for the modules in the project.
#[derive(Debug)]
pub struct Modules {
    /// An unsigned integer that specifies the number of elements in [`Modules::modules`].
    ///
    /// This value stores redundant information. It is populated by the parser and will
    /// always agree with `modules.len()`. It is kept here to accurately represent the
    /// raw binary contents.
    pub count: u16,
    /// Unused data. The value is populated by the parser but no longer used.
    pub cookie: u16,
    /// An array of [`Module`] records.
    pub modules: Vec<Module>,
}

/// Specifies the containing module's type.
#[derive(Debug)]
pub enum ModuleType {
    /// Specifies a procedural module.
    ///
    /// A procedural module is a collection of subroutines and functions.
    Procedural,
    /// Specifies a document module, class module, or designer module.
    ///
    /// A document module is a type of VBA project item that specifies a module for
    /// embedded macros and programmatic access operations that are associated with a
    /// document.
    ///
    /// A class module is a module that contains the definition for a new object. Each
    /// instance of a class creates a new object, and procedures that are defined in the
    /// module become properties and methods of the object.
    ///
    /// A designer module is a VBA module that extends the methods and properties of an
    /// ActiveX control that has been registered with the project.
    DocClsDesigner,
}

/// Specifies data for a module.
#[derive(Debug)]
pub struct Module {
    /// Specifies a VBA identifier as the name of the containing `Module`.
    pub name: String,
    /// Specifies a VBA identifier as the name of the containing `Module`.
    ///
    /// This field is optional in the file format specification. When present it
    /// is equal to the `name` field.
    pub name_unicode: Option<String>,
    pub stream_name: String,
    pub stream_name_unicode: String,
    pub doc_string: String,
    pub doc_string_unicode: String,
    pub text_offset: u32,
    pub help_context: u32,
    pub cookie: u16,
    /// Specifies whether the containing `Module` is a procedural module, document
    /// module, class module, or designer module.
    pub module_type: ModuleType,
    pub read_only: bool,
    pub private: bool,
}

impl Project {
    /// Returns a list of entries (storages and streams) in the raw binary data. Each
    /// entry is represented as a tuple of two `String`s, where the first element
    /// contains the entry's name and the second element the entry's path inside the
    /// CFB.
    ///
    /// The raw binary data is encoded as a [Compound File Binary](https://docs.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/53989ce4-7b05-4f8d-829b-d08d6148375b).
    pub fn list(&self) -> Result<Vec<(String, String)>> {
        let mut result = Vec::new();
        for entry in self.container.walk_storage("/").map_err(Error::Cfb)? {
            result.push((
                entry.name().to_owned(),
                entry.path().to_str().unwrap_or_default().to_owned(),
            ));
        }
        Ok(result)
    }

    pub fn read_stream(&mut self, stream_name: &str) -> Result<Vec<u8>> {
        let mut stream = self
            .container
            .open_stream(stream_name)
            .map_err(Error::Cfb)?;
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).map_err(Error::Cfb)?;

        Ok(buffer)
    }

    pub fn decompress_stream_from(&mut self, stream_name: &str, offset: usize) -> Result<Vec<u8>> {
        let data = self.read_stream(stream_name)?;
        let data = parser::decompress(&data[offset..])
            .map_err(|_| Error::Decompressor)?
            .1;
        Ok(data)
    }

    /// Returns version independent project information.
    pub fn information(&mut self) -> Result<ProjectInformation> {
        const DIR_STREAM_PATH: &str = r#"/VBA\dir"#;

        // Read *dir* stream
        let mut stream = self
            .container
            .open_stream(DIR_STREAM_PATH)
            .map_err(Error::Cfb)?;
        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).map_err(Error::Cfb)?;

        // Decompress stream
        let (remainder, buffer) = parser::decompress(&buffer).map_err(|_| Error::Decompressor)?;
        debug_assert!(remainder.is_empty());

        // Parse binary data
        let (remainder, information) =
            parser::parse_project_information(&buffer).map_err(|_| Error::Parser)?;
        debug_assert_eq!(remainder.len(), 0, "Stream not fully consumed");

        // Return structured information
        Ok(information)
    }
}

/// Constructs an opaque [`Project`] handle from raw binary data.
pub fn open_project(raw: Vec<u8>) -> Result<Project> {
    let cursor = Cursor::new(raw);
    let container = CompoundFile::open(cursor).map_err(Error::Cfb)?;

    Ok(Project { container })
}
