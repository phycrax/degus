use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use degus_controls::Controls;
use serde::de::DeserializeOwned;

use crate::error::{Error, Result, io_error};
use crate::layout::StructLayout;
use crate::model::{
    Control, Declarations, GroupSpec, IclSpec, PROJECT_FILE_NAME, PageSpec, RawDeclarations,
    RootFile, Settings, StructSpec, WaeSpec,
};
use crate::resolve::{Lookup, Resolved, Scope, resolve};

/// A loaded project: the settings of `project.toml`, and the declarations of
/// it and the files it includes.
#[derive(Debug, Clone)]
pub struct Project {
    file: PathBuf,
    root: PathBuf,
    pub settings: Settings,
    /// `project.toml`, then each file it includes, in the order `include`
    /// lists them. A file listed twice is here once; the check reports it.
    pub files: Vec<ProjectFile>,
}

/// A file of the project and what it declares.
#[derive(Debug, Clone)]
pub struct ProjectFile {
    /// Path relative to the project root.
    pub path: PathBuf,
    pub declarations: Declarations,
}

impl Project {
    /// Loads the project in the folder `dir`: its `project.toml` and every
    /// file it includes. Paths in the project are relative to `dir`.
    ///
    /// Controls take their styles here, so a control that does not parse,
    /// or names a style no file declares, fails the load.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self> {
        let root = dir.as_ref().to_path_buf();
        if root.is_file() {
            return Err(Error::NotAFolder(root));
        }
        let file = root.join(PROJECT_FILE_NAME);
        let (settings, declarations) = read_toml::<RootFile>(&file)?.split();
        let mut raw = vec![(PathBuf::from(PROJECT_FILE_NAME), declarations)];
        for path in &settings.include {
            if raw.iter().any(|(known, _)| known == path) {
                continue;
            }
            raw.push((path.clone(), read_toml(&root.join(path))?));
        }

        // A style declared in several files is the first one's.
        let mut styles = BTreeMap::new();
        for (_, declarations) in &raw {
            for (name, style) in &declarations.style {
                styles.entry(name.clone()).or_insert_with(|| style.clone());
            }
        }
        let files = raw
            .into_iter()
            .map(|(path, raw)| {
                let declarations = typed(&path, raw, &styles)?;
                Ok(ProjectFile { path, declarations })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            file,
            root,
            settings,
            files,
        })
    }

    /// The project file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The project folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `path` relative to the project root.
    pub fn resolve(&self, path: impl AsRef<Path>) -> PathBuf {
        self.root.join(path)
    }

    /// The folder [`Project::build`](crate::Project::build) writes `DWIN_SET`
    /// in.
    pub fn output_dir(&self) -> PathBuf {
        self.resolve(&self.settings.output)
    }

    /// The ICLs of every file.
    pub fn icls(&self) -> impl Iterator<Item = &IclSpec> {
        self.files.iter().flat_map(|file| &file.declarations.icl)
    }

    /// The WAEs of every file.
    pub fn waes(&self) -> impl Iterator<Item = &WaeSpec> {
        self.files.iter().flat_map(|file| &file.declarations.wae)
    }

    /// The files every file copies into the output.
    pub fn copies(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.iter().flat_map(|file| &file.declarations.copy)
    }

    /// The pages, each with its name and the index of its file in
    /// [`Project::files`]. A name declared in several files is the first
    /// one's; the check reports the others.
    pub fn pages(&self) -> impl Iterator<Item = (usize, &str, &PageSpec)> {
        self.files
            .iter()
            .enumerate()
            .flat_map(|(index, file)| {
                file.declarations
                    .page
                    .iter()
                    .map(move |(name, page)| (index, name.as_str(), page))
            })
            .filter(|&(index, name, _)| self.page(name).is_some_and(|(first, _)| first == index))
    }

    /// The page named `name`, with the index of its file in
    /// [`Project::files`].
    pub fn page(&self, name: &str) -> Option<(usize, &PageSpec)> {
        self.find(name, |declarations| &declarations.page)
    }

    /// The ID of the page named `name`.
    pub fn page_id(&self, name: &str) -> Option<u16> {
        self.page(name).map(|(_, page)| page.id)
    }

    /// The control named `name`, with the index of its file in
    /// [`Project::files`]. A name declared in several files is the first
    /// one's; the check reports the others.
    pub fn control(&self, name: &str) -> Option<(usize, &Control)> {
        self.find(name, |declarations| &declarations.control)
    }

    /// The group named `name`, with the index of its file.
    pub fn group(&self, name: &str) -> Option<(usize, &GroupSpec)> {
        self.find(name, |declarations| &declarations.group)
    }

    /// The struct named `name`, with the index of its file.
    pub fn struct_spec(&self, name: &str) -> Option<(usize, &StructSpec)> {
        self.find(name, |declarations| &declarations.structs)
    }

    /// The layout of every struct, by name, as far as it can be laid out;
    /// the check reports what cannot.
    pub fn layouts(&self) -> BTreeMap<&str, StructLayout> {
        let mut layouts = BTreeMap::new();
        for file in &self.files {
            for (name, spec) in &file.declarations.structs {
                layouts
                    .entry(name.as_str())
                    .or_insert_with(|| spec.layout().0);
            }
        }
        layouts
    }

    fn find<T>(
        &self,
        name: &str,
        map: impl Fn(&Declarations) -> &BTreeMap<String, T>,
    ) -> Option<(usize, &T)> {
        self.files
            .iter()
            .enumerate()
            .find_map(|(index, file)| map(&file.declarations).get(name).map(|t| (index, t)))
    }

    /// What `page` draws, in order: each control it lists, and for each
    /// group it lists, the group's controls once for each element of the
    /// struct it binds. Names nothing declares are left out.
    pub(crate) fn page_items<'a>(
        &'a self,
        page: &'a PageSpec,
        layouts: &BTreeMap<&str, StructLayout>,
    ) -> Vec<Item<'a>> {
        let mut items = Vec::new();
        for name in &page.controls {
            if let Some((file, control)) = self.control(name) {
                items.push(Item {
                    control,
                    scope: Scope::default(),
                    source: SourceRef {
                        file,
                        name: name.clone(),
                        group: None,
                    },
                });
            } else if let Some((file, group)) = self.group(name) {
                items.extend(group_items(file, name, group, layouts));
            }
        }
        items
    }

    /// The controls of every page, ready for `degus-controls` to build.
    /// Pages sharing an ID are merged, and what does not resolve is left out
    /// or written as 0; the check reports both.
    pub fn to_controls(&self) -> Controls {
        self.to_controls_with_sources().0
    }

    /// Like [`Project::to_controls`], with where each control comes from.
    pub fn to_controls_with_sources(&self) -> (Controls, SourceMap) {
        let layouts = self.layouts();
        let page_id = |name: &str| self.page_id(name);
        let lookup = Lookup {
            layouts: &layouts,
            page_id: &page_id,
        };
        let mut controls = Controls::new();
        let mut sources = SourceMap::default();
        for (_, _, page) in self.pages() {
            let items = self.page_items(page, &layouts);
            let target = controls.page_mut(page.id);
            let page_sources = sources.pages.entry(page.id).or_default();
            for item in items {
                match resolve(item.control, item.scope, &lookup).control {
                    Resolved::Touch(touch) => {
                        target.touch.push(touch);
                        page_sources.touch.push(item.source);
                    }
                    Resolved::Display(display) => {
                        target.display.push(display);
                        page_sources.display.push(item.source);
                    }
                }
            }
        }
        (controls, sources)
    }
}

/// The controls of `group`, copy by copy.
pub(crate) fn group_items<'a>(
    file: usize,
    name: &str,
    group: &'a GroupSpec,
    layouts: &BTreeMap<&str, StructLayout>,
) -> Vec<Item<'a>> {
    // A struct nothing declares binds nothing; the check reports it.
    let bound = group
        .bound
        .as_deref()
        .filter(|bound| layouts.contains_key(bound));
    let copies = bound
        .and_then(|bound| layouts.get(bound))
        .map_or(1, |layout| layout.count);
    let mut items = Vec::new();
    for copy in 0..copies {
        let scope = Scope {
            origin: (
                i64::from(group.origin.x) + i64::from(copy) * i64::from(group.step.x),
                i64::from(group.origin.y) + i64::from(copy) * i64::from(group.step.y),
            ),
            element: bound.map(|bound| (bound, copy)),
        };
        for (index, control) in group.controls.iter().enumerate() {
            items.push(Item {
                control,
                scope,
                source: SourceRef {
                    file,
                    name: name.to_owned(),
                    group: Some(GroupPlace {
                        control: index,
                        element: bound
                            .filter(|_| copies > 1)
                            .map(|bound| (bound.to_owned(), copy)),
                    }),
                },
            });
        }
    }
    items
}

/// A control a page draws: where its group's copy puts it, and where it is
/// declared.
pub(crate) struct Item<'a> {
    pub control: &'a Control,
    pub scope: Scope<'a>,
    pub source: SourceRef,
}

/// Builds a file's declarations from its tables, applying the styles.
fn typed(
    path: &Path,
    raw: RawDeclarations,
    styles: &BTreeMap<String, toml::Table>,
) -> Result<Declarations> {
    let path_text = path.display().to_string();
    let control = raw
        .control
        .into_iter()
        .map(|(name, table)| {
            let location = format!("{path_text}: [control.{name}]");
            Ok((name, build_control(&location, table, styles)?))
        })
        .collect::<Result<_>>()?;
    let group = raw
        .group
        .into_iter()
        .map(|(name, group)| {
            let controls = group
                .control
                .into_iter()
                .enumerate()
                .map(|(index, table)| {
                    let location = group_control_location(path, &name, index);
                    build_control(&location, table, styles)
                })
                .collect::<Result<_>>()?;
            let spec = GroupSpec {
                bound: group.bound,
                origin: group.origin,
                step: group.step,
                controls,
            };
            Ok((name, spec))
        })
        .collect::<Result<_>>()?;
    Ok(Declarations {
        copy: raw.copy,
        icl: raw.icl,
        wae: raw.wae,
        page: raw.page,
        structs: raw.structs,
        group,
        control,
        style: raw.style,
    })
}

/// A control from its table: its style's keys, then its own.
fn build_control(
    location: &str,
    mut table: toml::Table,
    styles: &BTreeMap<String, toml::Table>,
) -> Result<Control> {
    let error = |message: String| Error::Declaration {
        location: location.to_owned(),
        message,
    };
    let mut merged = match table.remove("style") {
        None => toml::Table::new(),
        Some(toml::Value::String(name)) => {
            let style = styles
                .get(&name)
                .ok_or_else(|| error(format!("style {name} is not declared")))?;
            if style.contains_key("style") {
                return Err(error(format!(
                    "style {name} names a style itself; a style cannot use another"
                )));
            }
            style.clone()
        }
        Some(_) => return Err(error("style takes a style's name".to_owned())),
    };
    merged.extend(table);
    toml::Value::Table(merged)
        .try_into()
        .map_err(|source: toml::de::Error| error(source.message().to_owned()))
}

/// `pages/logs.toml: [group.log_row] control 2`, numbered from 1.
pub(crate) fn group_control_location(path: &Path, group: &str, index: usize) -> String {
    format!("{}: [group.{group}] control {}", path.display(), index + 1)
}

/// Where each control of [`Project::to_controls_with_sources`] comes from,
/// page by page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMap {
    pub pages: BTreeMap<u16, PageSources>,
}

impl SourceMap {
    /// The sources of a page's controls, if it has any.
    pub fn page(&self, id: u16) -> Option<&PageSources> {
        self.pages.get(&id)
    }
}

/// The sources of one page's controls, in the order of its
/// [`PageControls`](degus_controls::PageControls): `touch[i]` is where
/// touch control `i` comes from, and the same for `display`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageSources {
    pub touch: Vec<SourceRef>,
    pub display: Vec<SourceRef>,
}

/// A control as a file declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    /// Index into [`Project::files`] of the file declaring it.
    pub file: usize,
    /// The control's name, or the name of its group.
    pub name: String,
    /// For a control in a group, where in the group.
    pub group: Option<GroupPlace>,
}

/// A control in a group's copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPlace {
    /// The control's place in the group, from 0.
    pub control: usize,
    /// The struct element the copy draws, when the group repeats.
    pub element: Option<(String, u16)>,
}

impl SourceRef {
    /// Where the control is declared, as check messages name it, with
    /// `path` the path of its file: `controls/home.toml: [control.fan_on]`,
    /// or `pages/logs.toml: [group.log_row] control 2 (log_row[3])`.
    pub fn location(&self, path: &Path) -> String {
        match &self.group {
            None => format!("{}: [control.{}]", path.display(), self.name),
            Some(place) => {
                let mut location = group_control_location(path, &self.name, place.control);
                if let Some((bound, index)) = &place.element {
                    location.push_str(&format!(" ({bound}[{index}])"));
                }
                location
            }
        }
    }
}

fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(io_error(path))?;
    toml::from_str(&text).map_err(|source| Error::Toml {
        path: path.to_path_buf(),
        source,
    })
}
