//! Raw inventory + world → snapshot (v0.1 spec §7 and §8).
//!
//! 1. The opened project is the git root of the launch folder (or the folder itself).
//! 2. A process belongs to a project when its working folder is inside one of the
//!    project's worktrees; a process whose folder is unreadable inherits from its
//!    nearest ancestor. A container belongs to the project that contains its compose
//!    `working_dir`.
//! 3. Each listening socket gets an owner: a process, a container (through an engine
//!    forwarder), the system, or "unknown" when the owner is not visible.
//! 4. Listener owners inside projects become services, grouped by the top of their
//!    wrapper chain (`npm run dev` → `sh -c` → `vite`). Background workers become
//!    services too. Editors and AI agents never do.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use hm_core::ids::{project_id, service_id};
use hm_core::model::{
    BindScope, Compose, Confidence, Container, Kind, Listener, Note, Owner, Process, Project, Role,
    SNAPSHOT_SCHEMA, Section, Service, ServiceSource, Snapshot, Worktree, canonical_ip,
};
use hm_core::names::exe_stem;
use hm_core::raw::{RawContainer, RawInventory, RawListener, RawProcess};
use hm_core::signature::{Signature, SignatureSet};

use crate::manifest::{Manifest, read_manifest};
use crate::matcher::{Match, ProcFacts, match_image, match_process, script_stem};
use crate::paths::{is_boundary, within};
use crate::world::World;

pub struct ResolveInput<'a> {
    pub inventory: &'a RawInventory,
    pub world: &'a dyn World,
    pub signatures: &'a SignatureSet,
    /// The folder Harbormap was started in.
    pub launch_dir: &'a Path,
}

/// Programs that only start another program: the service is the whole chain.
const WRAPPER_EXES: &[&str] = &[
    "npm", "npx", "pnpm", "pnpx", "yarn", "bun", "bunx", "uv", "uvx", "poetry", "pipenv", "pdm",
    "hatch", "rye", "dotnet", "go", "cargo", "air", "nodemon",
];
/// Node scripts that act as wrappers when run as `node <script>`.
const WRAPPER_SCRIPTS: &[&str] = &[
    "npm-cli",
    "npx-cli",
    "yarn",
    "pnpm",
    "nodemon",
    "tsx",
    "ts-node-dev",
    "corepack",
];
/// Shells count as wrappers only when they run a single command (`sh -c ...`).
const SHELLS: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "cmd",
    "powershell",
    "pwsh",
];
const MAVEN_LAUNCHER: &str = "org.codehaus.plexus.classworlds";
/// Ends every "unknown owner" reason that root or admin rights would reveal.
const NEEDS_ADMIN: &str = "seeing it needs admin rights";
const COMPOSE: &str = "com.docker.compose.";

pub fn build_snapshot(input: ResolveInput<'_>) -> Snapshot {
    Resolver::new(input).run()
}

struct ProjectCtx {
    id: String,
    name: String,
    root: PathBuf,
    git: bool,
    opened: bool,
    worktrees: Vec<Worktree>,
}

impl ProjectCtx {
    fn new(root: PathBuf, git: bool, opened: bool, worktrees: Vec<Worktree>) -> Self {
        let name = folder_name(&root);
        ProjectCtx {
            id: project_id(&root),
            name,
            root,
            git,
            opened,
            worktrees,
        }
    }
}

/// Where something lives: project index plus the worktree path that contains it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Place {
    project: usize,
    worktree: PathBuf,
}

struct Resolver<'a> {
    inv: &'a RawInventory,
    world: &'a dyn World,
    signatures: &'a SignatureSet,
    windows: bool,
    home: Option<PathBuf>,
    projects: Vec<ProjectCtx>,
    procs: BTreeMap<u32, &'a RawProcess>,
    children: BTreeMap<u32, Vec<u32>>,
    places: HashMap<u32, Place>,
    matches: HashMap<u32, Match<'a>>,
    manifests: HashMap<(PathBuf, PathBuf), Manifest>,
}

/// A service before naming and deduplication.
struct Draft<'a> {
    place: Option<Place>,
    signature: Option<Match<'a>>,
    source: DraftSource,
    ports: BTreeSet<u16>,
    base_name: String,
    state: String,
    health: Option<String>,
}

enum DraftSource {
    Process {
        main: u32,
        root: u32,
        pids: Vec<u32>,
    },
    Container {
        id: String,
    },
}

impl<'a> Resolver<'a> {
    fn new(input: ResolveInput<'a>) -> Self {
        let world = input.world;
        let windows = world.windows() || input.inventory.os == "windows";
        let mut procs = BTreeMap::new();
        let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for process in &input.inventory.processes {
            procs.insert(process.pid, process);
            if let Some(ppid) = process.ppid
                && ppid != process.pid
            {
                children.entry(ppid).or_default().push(process.pid);
            }
        }
        let mut resolver = Resolver {
            inv: input.inventory,
            world,
            signatures: input.signatures,
            windows,
            home: world.home(),
            projects: Vec::new(),
            procs,
            children,
            places: HashMap::new(),
            matches: HashMap::new(),
            manifests: HashMap::new(),
        };
        resolver.open(input.launch_dir);
        resolver
    }

    /// Step 1: the opened project.
    fn open(&mut self, launch_dir: &Path) {
        let dir = self.world.canonicalize(launch_dir);
        if let Some(repo) = self.world.repo_for(&dir)
            && !is_boundary(&repo.root, self.home.as_deref(), self.windows)
        {
            self.projects
                .push(ProjectCtx::new(repo.root, true, true, repo.worktrees));
        } else if !is_boundary(&dir, self.home.as_deref(), self.windows) {
            self.projects.push(ProjectCtx::new(
                dir.clone(),
                false,
                true,
                vec![Worktree {
                    path: dir,
                    branch: None,
                    head: None,
                }],
            ));
        }
    }

    fn run(mut self) -> Snapshot {
        self.place_processes();
        self.match_processes();
        let containers = self.place_containers();
        let inv = self.inv;
        let listeners: Vec<(&RawListener, Owner)> = inv
            .listeners
            .iter()
            .map(|listener| (listener, self.owner(listener, &containers)))
            .collect();
        let services = self.services(&listeners, &containers);
        self.assemble(listeners, containers, services)
    }

    /// Step 2a: project membership by working folder.
    fn place_processes(&mut self) {
        let pids: Vec<u32> = self.procs.keys().copied().collect();
        for pid in &pids {
            if *pid == self.inv.self_pid {
                continue;
            }
            let cwd = self.procs[pid].cwd.clone();
            if let Some(cwd) = cwd
                && let Some(place) = self.locate(&cwd)
            {
                self.places.insert(*pid, place);
            }
        }
        // Unreadable working folders inherit from the nearest placed ancestor.
        for pid in &pids {
            if *pid == self.inv.self_pid || self.procs[pid].cwd.is_some() {
                continue;
            }
            if let Some(place) = self
                .ancestors(*pid)
                .into_iter()
                .take(8)
                .find_map(|ancestor| self.places.get(&ancestor).cloned())
            {
                self.places.insert(*pid, place);
            }
        }
    }

    /// Step 2b: signatures for listener owners and for everything inside projects.
    fn match_processes(&mut self) {
        let mut wanted: BTreeSet<u32> = self.places.keys().copied().collect();
        wanted.extend(
            self.inv
                .listeners
                .iter()
                .flat_map(|l| l.pids.iter().copied()),
        );
        for pid in wanted {
            let Some(process) = self.procs.get(&pid).copied() else {
                continue;
            };
            let facts = self.facts(process);
            if let Some(found) = match_process(self.signatures, &facts) {
                self.matches.insert(pid, found);
            }
        }
        // A dev program outside git but under the home folder gets a folder project.
        // Shallow folders first, so a folder inside another joins it whatever the pids.
        let mut owners: Vec<u32> = self
            .inv
            .listeners
            .iter()
            .flat_map(|l| l.pids.iter().copied())
            .collect::<BTreeSet<u32>>()
            .into_iter()
            .collect();
        owners.sort_by_key(|pid| {
            let depth = self
                .procs
                .get(pid)
                .and_then(|process| process.cwd.as_ref())
                .map_or(0, |cwd| cwd.components().count());
            (depth, *pid)
        });
        for pid in owners {
            if self.places.contains_key(&pid) || pid == self.inv.self_pid {
                continue;
            }
            let (Some(process), Some(found)) =
                (self.procs.get(&pid).copied(), self.matches.get(&pid))
            else {
                continue;
            };
            let dev = matches!(
                found.signature.kind,
                Kind::DevServer | Kind::Database | Kind::Cache | Kind::Worker
            );
            let own = process.uid.is_some() && process.uid == self.inv.current_uid;
            if let (true, true, Some(cwd)) = (dev, own, process.cwd.clone())
                && self.under_home(&cwd)
            {
                let place = self.folder_project(&cwd);
                self.places.insert(pid, place);
            }
        }
    }

    fn facts(&mut self, process: &RawProcess) -> ProcFacts {
        let exe = process
            .exe
            .as_ref()
            .map(|exe| exe_stem(&exe.to_string_lossy()))
            .unwrap_or_else(|| exe_stem(&process.name));
        let ancestors = self
            .ancestors(process.pid)
            .into_iter()
            .take(16)
            .filter_map(|pid| self.procs.get(&pid))
            .map(|ancestor| stem_of(ancestor))
            .collect();
        let place = self.places.get(&process.pid).cloned();
        let readable_cwd = process
            .cwd
            .clone()
            .filter(|cwd| place.is_some() || self.under_home(cwd));
        let (cwd_files, packages) = match readable_cwd {
            Some(cwd) => {
                let stop = place.map_or_else(|| cwd.clone(), |place| place.worktree);
                let packages = self.manifest(&cwd, &stop).packages;
                (self.world.file_names(&cwd), packages)
            }
            None => (Vec::new(), BTreeSet::new()),
        };
        ProcFacts {
            exe,
            name: exe_stem(&process.name),
            argv: process.argv.clone(),
            ancestors,
            cwd_files,
            packages,
            os: self.inv.os.clone(),
        }
    }

    /// Step 2c: containers by compose working folder.
    fn place_containers(&mut self) -> Vec<(&'a RawContainer, Option<Place>, Option<Match<'a>>)> {
        let inv = self.inv;
        let mut placed = Vec::with_capacity(inv.containers.len());
        for container in &inv.containers {
            let working_dir = container
                .labels
                .get(&format!("{COMPOSE}project.working_dir"))
                // Compose keeps the folder as it was typed, symlinks included (on macOS,
                // /tmp is /private/tmp), while project roots are canonical.
                .map(|dir| self.world.canonicalize(Path::new(dir)));
            let place = match working_dir {
                Some(dir) => match self.locate(&dir) {
                    Some(place) => Some(place),
                    None if !is_boundary(&dir, self.home.as_deref(), self.windows) => {
                        Some(self.folder_project(&dir))
                    }
                    None => None,
                },
                None => None,
            };
            let found = match_image(self.signatures, &container.image, &inv.os);
            placed.push((container, place, found));
        }
        placed
    }

    /// Step 3: who holds a listening socket.
    fn owner(
        &self,
        listener: &RawListener,
        containers: &[(&RawContainer, Option<Place>, Option<Match<'_>>)],
    ) -> Owner {
        let forwarded = || container_for_port(listener, containers);
        if listener.pids.is_empty() {
            return match forwarded() {
                Some(container_id) => Owner::ContainerForwarder { container_id },
                None => Owner::Unknown {
                    reason: match listener.uid {
                        Some(0) => format!("it belongs to the system (root); {NEEDS_ADMIN}"),
                        Some(uid) if self.inv.current_uid.as_deref() == Some(&*uid.to_string()) => {
                            format!(
                                "it belongs to your own user (uid {uid}), but its program is hidden; {NEEDS_ADMIN}"
                            )
                        }
                        Some(uid) => {
                            format!("it belongs to another user (uid {uid}); {NEEDS_ADMIN}")
                        }
                        None => NEEDS_ADMIN.to_string(),
                    },
                },
            };
        }
        if listener.pids == [0] {
            return Owner::System {
                reason: "the operating system kernel".into(),
                pid: None,
            };
        }
        let Some(main) = self.main_pid(&listener.pids) else {
            return Owner::Unknown {
                reason: "the program ended before Harbormap could look at it".into(),
            };
        };
        match self.matches.get(&main).map(|found| found.signature) {
            Some(signature) if signature.forwarder => match forwarded() {
                Some(container_id) => Owner::ContainerForwarder { container_id },
                None => Owner::System {
                    reason: format!("{} (no container publishes this port)", signature.name),
                    pid: Some(main),
                },
            },
            Some(signature) if signature.kind == Kind::System => Owner::System {
                reason: signature.name.clone(),
                pid: Some(main),
            },
            _ => Owner::Process { pid: main },
        }
    }

    /// The topmost visible process among those sharing a socket.
    fn main_pid(&self, pids: &[u32]) -> Option<u32> {
        let visible: Vec<u32> = pids
            .iter()
            .copied()
            .filter(|pid| self.procs.contains_key(pid))
            .collect();
        visible
            .iter()
            .copied()
            .filter(|pid| {
                self.procs[pid]
                    .ppid
                    .is_none_or(|ppid| !visible.contains(&ppid))
            })
            .min()
            .or_else(|| visible.iter().copied().min())
    }

    /// Step 4: services.
    fn services(
        &mut self,
        listeners: &[(&RawListener, Owner)],
        containers: &[(&'a RawContainer, Option<Place>, Option<Match<'a>>)],
    ) -> Vec<Draft<'a>> {
        let mut by_root: BTreeMap<u32, Draft<'a>> = BTreeMap::new();
        let mut owner_ports: BTreeMap<u32, BTreeSet<u16>> = BTreeMap::new();
        for (listener, owner) in listeners {
            if let Owner::Process { pid } = owner {
                owner_ports.entry(*pid).or_default().insert(listener.port);
            }
        }
        // Background workers have no ports but are services too.
        let workers: Vec<u32> = self
            .places
            .keys()
            .copied()
            .filter(|pid| {
                self.matches
                    .get(pid)
                    .is_some_and(|found| found.signature.kind == Kind::Worker)
            })
            .collect();
        // Every program that is a service of its own; a wrapper they share belongs to
        // none of them.
        let mut owners: BTreeSet<u32> = owner_ports
            .keys()
            .copied()
            .filter(|pid| self.can_be_service(*pid))
            .collect();
        owners.extend(workers.iter().copied());
        for (pid, ports) in owner_ports {
            if !self.can_be_service(pid) {
                continue;
            }
            let root = self.service_root(pid, &owners);
            let draft = by_root
                .entry(root)
                .or_insert_with(|| self.process_draft(pid, root));
            draft.ports.extend(ports);
        }
        let mut covered: BTreeSet<u32> = BTreeSet::new();
        for root in by_root.keys() {
            covered.extend(self.descendants(*root));
        }
        let mut workers = workers;
        // Ancestors first, so a pool's master takes in its children whatever their pids.
        workers.sort_unstable_by_key(|pid| (self.depth(*pid), *pid));
        for pid in workers {
            if covered.contains(&pid) {
                continue;
            }
            let root = self.service_root(pid, &owners);
            covered.extend(self.descendants(root));
            by_root
                .entry(root)
                .or_insert_with(|| self.process_draft(pid, root));
        }

        let roots: BTreeSet<u32> = by_root.keys().copied().collect();
        let mut drafts: Vec<Draft<'a>> = by_root.into_values().collect();
        for draft in &mut drafts {
            if let DraftSource::Process { main, root, pids } = &mut draft.source {
                // A process belongs to the nearest service root above it, so a server
                // inside another service's tree is counted once, in its own service.
                *pids = self
                    .descendants(*root)
                    .into_iter()
                    .filter(|pid| self.nearest_root(*pid, &roots) == Some(*root))
                    .collect();
                let (main, place) = (*main, draft.place.clone());
                draft.base_name = self.process_service_name(main, place.as_ref(), draft.signature);
            }
        }
        for (container, place, found) in containers {
            drafts.push(Draft {
                place: place.clone(),
                signature: *found,
                source: DraftSource::Container {
                    id: container.id.clone(),
                },
                ports: container
                    .ports
                    .iter()
                    .filter_map(|binding| binding.host_port)
                    .collect(),
                base_name: container_service_name(container),
                state: container.state.clone(),
                health: container.health.clone(),
            });
        }
        drafts
    }

    /// A process service whose main process is `pid` and whose tree starts at `root`.
    fn process_draft(&self, pid: u32, root: u32) -> Draft<'a> {
        Draft {
            place: self.places.get(&pid).cloned(),
            signature: self.matches.get(&pid).copied(),
            source: DraftSource::Process {
                main: pid,
                root,
                pids: Vec::new(),
            },
            ports: BTreeSet::new(),
            base_name: String::new(),
            state: "running".into(),
            health: None,
        }
    }

    fn can_be_service(&self, pid: u32) -> bool {
        if pid == self.inv.self_pid || !self.places.contains_key(&pid) {
            return false;
        }
        !self.matches.get(&pid).is_some_and(|found| {
            matches!(
                found.signature.kind,
                Kind::Ide | Kind::Terminal | Kind::AgentCli | Kind::System
            )
        })
    }

    /// Walks up through wrappers and same-signature parents inside the same project, but
    /// not into a parent whose tree also holds another service's program: two servers
    /// started by one script stay two services.
    fn service_root(&self, pid: u32, owners: &BTreeSet<u32>) -> u32 {
        let own: BTreeSet<u32> = self.descendants(pid).into_iter().collect();
        let mut current = pid;
        for _ in 0..16 {
            let Some(parent) = self.procs[&current]
                .ppid
                .and_then(|ppid| self.procs.get(&ppid).copied())
            else {
                break;
            };
            let same_project = self.places.get(&parent.pid).map(|place| place.project)
                == self.places.get(&current).map(|place| place.project);
            let same_signature = match (self.matches.get(&parent.pid), self.matches.get(&current)) {
                (Some(a), Some(b)) => a.signature.id == b.signature.id,
                _ => false,
            };
            if parent.pid == self.inv.self_pid
                || !same_project
                || !(is_wrapper(parent) || same_signature)
            {
                break;
            }
            let shared = self
                .descendants(parent.pid)
                .into_iter()
                .any(|other| owners.contains(&other) && !own.contains(&other));
            if shared {
                break;
            }
            current = parent.pid;
        }
        current
    }

    /// How many known processes sit above `pid`.
    fn depth(&self, pid: u32) -> usize {
        let mut depth = 0;
        let mut current = pid;
        while depth < 64 {
            match self.procs.get(&current).and_then(|process| process.ppid) {
                Some(ppid) if self.procs.contains_key(&ppid) => {
                    depth += 1;
                    current = ppid;
                }
                _ => break,
            }
        }
        depth
    }

    /// The service root at `pid` or nearest above it.
    fn nearest_root(&self, pid: u32, roots: &BTreeSet<u32>) -> Option<u32> {
        let mut current = pid;
        for _ in 0..64 {
            if roots.contains(&current) {
                return Some(current);
            }
            current = self.procs.get(&current)?.ppid?;
        }
        None
    }

    /// The project's own programs (servers, databases, workers) are named after their
    /// manifest or folder. Anything else is named after what it is: its signature
    /// (`headless-browser`) or, when unrecognised, its program name (`google-chrome`).
    fn process_service_name(
        &mut self,
        main: u32,
        place: Option<&Place>,
        found: Option<Match<'_>>,
    ) -> String {
        let process = self.procs[&main];
        let own_program = found.is_some_and(|found| {
            matches!(
                found.signature.kind,
                Kind::DevServer | Kind::Database | Kind::Cache | Kind::Worker
            )
        });
        if own_program && let (Some(cwd), Some(place)) = (process.cwd.clone(), place) {
            if let Some(name) = self.manifest(&cwd, &place.worktree).name {
                return name;
            }
            if !within(&place.worktree, &cwd, self.windows) {
                return folder_name(&cwd);
            }
        }
        match found {
            Some(found) => found.signature.id.clone(),
            None => slug(&stem_of(process)),
        }
    }

    fn assemble(
        self,
        listeners: Vec<(&RawListener, Owner)>,
        containers: Vec<(&RawContainer, Option<Place>, Option<Match<'_>>)>,
        drafts: Vec<Draft<'_>>,
    ) -> Snapshot {
        let section_of = |place: Option<&Place>| match place {
            Some(place) if self.projects[place.project].opened => Section::Project,
            _ => Section::Elsewhere,
        };

        // Names are unique within one project and worktree. Services with ports pick
        // first. A different program that wants a taken name gets its signature as a
        // suffix ("shop-api-celery"); another copy of the same program gets "(2)".
        let mut order: Vec<usize> = (0..drafts.len()).collect();
        order.sort_by(|&a, &b| draft_key(&drafts[a]).cmp(&draft_key(&drafts[b])));
        type Scope = (Option<usize>, Option<PathBuf>, String);
        let mut taken: BTreeMap<Scope, Option<&str>> = BTreeMap::new();
        let mut names = vec![String::new(); drafts.len()];
        for index in order {
            let draft = &drafts[index];
            let project = draft.place.as_ref().map(|place| place.project);
            let worktree = draft.place.as_ref().map(|place| place.worktree.clone());
            let key = |name: &str| (project, worktree.clone(), name.to_string());
            let signature = draft.signature.map(|found| found.signature.id.as_str());
            let base = &draft.base_name;
            let mut candidates = vec![base.clone()];
            if let Some(holder) = taken.get(&key(base))
                && *holder != signature
                && let Some(id) = signature
            {
                candidates.push(format!("{base}-{id}"));
            }
            candidates.extend((2..66).map(|n| format!("{base} ({n})")));
            let name = candidates
                .into_iter()
                .find(|candidate| !taken.contains_key(&key(candidate)))
                .unwrap_or_else(|| base.clone());
            taken.insert(key(&name), signature);
            names[index] = name;
        }

        let mut services: Vec<Service> = drafts
            .iter()
            .zip(names)
            .map(|(draft, name)| {
                let project = draft
                    .place
                    .as_ref()
                    .map(|place| &self.projects[place.project]);
                let signature: Option<&Signature> = draft.signature.map(|found| found.signature);
                let http = signature.is_some_and(|signature| signature.http);
                Service {
                    id: service_id(
                        project.map(|project| project.id.as_str()),
                        draft.place.as_ref().map(|place| place.worktree.as_path()),
                        &name,
                    ),
                    name,
                    section: section_of(draft.place.as_ref()),
                    project_id: project.map(|project| project.id.clone()),
                    worktree: draft.place.as_ref().map(|place| place.worktree.clone()),
                    signature: signature.map(|signature| signature.id.clone()),
                    framework: signature.map(|signature| signature.name.clone()),
                    confidence: draft
                        .signature
                        .map_or(Confidence::Sure, |found| found.confidence),
                    role: signature.map_or(Role::Unknown, |signature| signature.role),
                    kind: signature.map_or(Kind::Unknown, |signature| signature.kind),
                    source: match &draft.source {
                        DraftSource::Process { main, root, pids } => ServiceSource::Process {
                            main_pid: *main,
                            root_pid: *root,
                            pids: pids.clone(),
                        },
                        DraftSource::Container { id } => ServiceSource::Container {
                            container_id: id.clone(),
                        },
                    },
                    ports: draft.ports.iter().copied().collect(),
                    urls: if http {
                        draft
                            .ports
                            .iter()
                            .map(|port| format!("http://localhost:{port}"))
                            .collect()
                    } else {
                        Vec::new()
                    },
                    state: draft.state.clone(),
                    health: draft.health.clone(),
                }
            })
            .collect();
        services.sort_by_cached_key(|service| {
            (
                service.section,
                self.project_name(service.project_id.as_deref())
                    .to_lowercase(),
                service.name.to_lowercase(),
                service.id.clone(),
            )
        });

        let service_of_pid: HashMap<u32, &Service> = services
            .iter()
            .flat_map(|service| match &service.source {
                ServiceSource::Process { pids, .. } => {
                    pids.iter().map(|pid| (*pid, service)).collect::<Vec<_>>()
                }
                ServiceSource::Container { .. } => Vec::new(),
            })
            .collect();
        let service_of_container: HashMap<&str, &Service> = services
            .iter()
            .filter_map(|service| match &service.source {
                ServiceSource::Container { container_id } => Some((container_id.as_str(), service)),
                ServiceSource::Process { .. } => None,
            })
            .collect();

        let mut hidden_owners = 0;
        let mut out_listeners: Vec<Listener> = listeners
            .into_iter()
            .map(|(raw, owner)| {
                let service = match &owner {
                    Owner::Process { pid } => service_of_pid.get(pid).copied(),
                    Owner::ContainerForwarder { container_id } => {
                        service_of_container.get(container_id.as_str()).copied()
                    }
                    _ => None,
                };
                let label = match (&owner, service) {
                    // "web (Vite)", or just "Headless browser" when the name is the signature.
                    (_, Some(service)) => match &service.framework {
                        Some(framework)
                            if service.signature.as_deref() == Some(service.name.as_str()) =>
                        {
                            framework.clone()
                        }
                        Some(framework) => format!("{} ({framework})", service.name),
                        None => service.name.clone(),
                    },
                    (Owner::Process { pid }, None) => {
                        let what = self.matches.get(pid).map_or_else(
                            || self.procs[pid].name.clone(),
                            |found| match found.confidence {
                                Confidence::Probably => {
                                    format!("{}, probably", found.signature.name)
                                }
                                Confidence::Sure => found.signature.name.clone(),
                            },
                        );
                        format!("{what} (pid {pid})")
                    }
                    (Owner::System { reason, .. }, None) => reason.clone(),
                    (Owner::ContainerForwarder { container_id }, None) => {
                        format!("container {}", short_id(container_id))
                    }
                    (Owner::Unknown { reason }, None) => {
                        if reason.ends_with(NEEDS_ADMIN) {
                            hidden_owners += 1;
                        }
                        "unknown owner".into()
                    }
                };
                let addr = canonical_ip(raw.addr);
                Listener {
                    proto: raw.proto,
                    addr,
                    port: raw.port,
                    bind: BindScope::of(addr),
                    section: service.map_or(Section::System, |service| service.section),
                    project_id: service.and_then(|service| service.project_id.clone()),
                    service_id: service.map(|service| service.id.clone()),
                    label,
                    owner,
                }
            })
            .collect();
        out_listeners.sort_by_key(|listener| (listener.port, listener.addr));

        let mut keep: BTreeSet<u32> = service_of_pid.keys().copied().collect();
        for listener in &out_listeners {
            let owner_pid = match listener.owner {
                Owner::Process { pid } | Owner::System { pid: Some(pid), .. } => Some(pid),
                _ => None,
            };
            if let Some(pid) = owner_pid {
                keep.insert(pid);
                if let Some(ppid) = self.procs.get(&pid).and_then(|process| process.ppid) {
                    keep.insert(ppid);
                }
            }
        }
        let processes: Vec<Process> = keep
            .into_iter()
            .filter_map(|pid| self.procs.get(&pid).copied())
            .map(|raw| {
                let place = self.places.get(&raw.pid);
                let found = self.matches.get(&raw.pid);
                Process {
                    pid: raw.pid,
                    ppid: raw.ppid,
                    start_time: raw.start_time,
                    name: raw.name.clone(),
                    exe: raw.exe.clone(),
                    argv: raw.argv.clone(),
                    cwd: raw.cwd.clone(),
                    user: raw.user.clone(),
                    own: raw.uid.is_some() && raw.uid == self.inv.current_uid,
                    signature: found.map(|found| found.signature.id.clone()),
                    confidence: found.map_or(Confidence::Sure, |found| found.confidence),
                    kind: found.map_or(Kind::Unknown, |found| found.signature.kind),
                    project_id: place.map(|place| self.projects[place.project].id.clone()),
                    worktree: place.map(|place| place.worktree.clone()),
                    rss_bytes: raw.rss_bytes,
                    uptime_s: self.inv.taken_at.saturating_sub(raw.start_time),
                    env: raw.env.clone(),
                }
            })
            .collect();

        let mut out_containers: Vec<Container> = containers
            .iter()
            .map(|(raw, place, found)| Container {
                id: raw.id.clone(),
                engine: raw.engine.clone(),
                name: raw.name.clone(),
                image: raw.image.clone(),
                state: raw.state.clone(),
                health: raw.health.clone(),
                exit_code: raw.exit_code,
                oom_killed: raw.oom_killed,
                restart_count: raw.restart_count,
                started_at: raw.started_at.clone(),
                ports: raw.ports.clone(),
                networks: raw.networks.clone(),
                mounts: raw.mounts.clone(),
                compose: compose_of(raw),
                signature: found.map(|found| found.signature.id.clone()),
                section: section_of(place.as_ref()),
                project_id: place
                    .as_ref()
                    .map(|place| self.projects[place.project].id.clone()),
            })
            .collect();
        out_containers.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));

        let used: BTreeSet<&str> = services
            .iter()
            .filter_map(|service| service.project_id.as_deref())
            .collect();
        let mut projects: Vec<Project> = self
            .projects
            .iter()
            .filter(|project| project.opened || used.contains(project.id.as_str()))
            .map(|project| Project {
                id: project.id.clone(),
                name: project.name.clone(),
                root: project.root.clone(),
                opened: project.opened,
                git: project.git,
                worktrees: project.worktrees.clone(),
            })
            .collect();
        projects.sort_by(|a, b| (!a.opened, &a.name, &a.root).cmp(&(!b.opened, &b.name, &b.root)));

        let mut engines = self.inv.engines.clone();
        engines.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));

        let mut notes = self.inv.notes.clone();
        let opened = self.projects.iter().find(|project| project.opened);
        if opened.is_none() {
            notes.push(Note {
                code: "not_in_project".into(),
                text: "This folder is not a project, so Harbormap shows the whole computer.".into(),
            });
        }
        if hidden_owners > 0 {
            let text = if hidden_owners == 1 {
                "1 listening port belongs to a program Harbormap can't see without admin rights."
                    .to_string()
            } else {
                format!(
                    "{hidden_owners} listening ports belong to programs Harbormap can't see without admin rights."
                )
            };
            notes.push(Note {
                code: "owners_hidden".into(),
                text,
            });
        }

        Snapshot {
            schema: SNAPSHOT_SCHEMA,
            taken_at: self.inv.taken_at,
            host: hm_core::model::Host {
                os: self.inv.os.clone(),
                arch: self.inv.arch.clone(),
            },
            opened: opened.map(|project| project.id.clone()),
            projects,
            services,
            listeners: out_listeners,
            processes,
            containers: out_containers,
            engines,
            notes,
        }
    }

    // ----- helpers -----

    /// The project and worktree of `dir`. The repository around `dir` is always looked
    /// up, so a repository is its own project even inside a folder opened without git or
    /// inside another repository (a nested clone), whatever order processes come in.
    fn locate(&mut self, dir: &Path) -> Option<Place> {
        if let Some(repo) = self.world.repo_for(dir)
            && !is_boundary(&repo.root, self.home.as_deref(), self.windows)
        {
            match self.project_at(&repo.root) {
                Some(index) if self.projects[index].git => {}
                // A folder project at the root of a bare repository's holder folder
                // (`mono/.bare`) is that repository: one project, one id.
                Some(index) => {
                    let project = &mut self.projects[index];
                    project.git = true;
                    project.worktrees.extend(repo.worktrees);
                }
                None => self
                    .projects
                    .push(ProjectCtx::new(repo.root, true, false, repo.worktrees)),
            }
        }
        self.find_known(dir)
    }

    /// The project whose root is `root`.
    fn project_at(&self, root: &Path) -> Option<usize> {
        self.projects.iter().position(|project| {
            within(&project.root, root, self.windows) && within(root, &project.root, self.windows)
        })
    }

    fn find_known(&self, dir: &Path) -> Option<Place> {
        let mut best: Option<(usize, &Worktree)> = None;
        for (index, project) in self.projects.iter().enumerate() {
            for worktree in &project.worktrees {
                let deeper = best.is_none_or(|(_, current)| {
                    worktree.path.as_os_str().len() > current.path.as_os_str().len()
                });
                if deeper && within(dir, &worktree.path, self.windows) {
                    best = Some((index, worktree));
                }
            }
        }
        best.map(|(project, worktree)| Place {
            project,
            worktree: worktree.path.clone(),
        })
    }

    fn folder_project(&mut self, dir: &Path) -> Place {
        if let Some(place) = self.find_known(dir) {
            return place;
        }
        // The holder folder of a bare repository is already that project's root.
        if let Some(index) = self.project_at(dir) {
            self.projects[index].worktrees.push(Worktree {
                path: dir.to_path_buf(),
                branch: None,
                head: None,
            });
            return Place {
                project: index,
                worktree: dir.to_path_buf(),
            };
        }
        self.projects.push(ProjectCtx::new(
            dir.to_path_buf(),
            false,
            false,
            vec![Worktree {
                path: dir.to_path_buf(),
                branch: None,
                head: None,
            }],
        ));
        Place {
            project: self.projects.len() - 1,
            worktree: dir.to_path_buf(),
        }
    }

    fn under_home(&self, dir: &Path) -> bool {
        self.home.as_ref().is_some_and(|home| {
            within(dir, home, self.windows) && !is_boundary(dir, Some(home), self.windows)
        })
    }

    fn manifest(&mut self, dir: &Path, stop: &Path) -> Manifest {
        let key = (dir.to_path_buf(), stop.to_path_buf());
        if let Some(found) = self.manifests.get(&key) {
            return found.clone();
        }
        let manifest = read_manifest(self.world, dir, stop);
        self.manifests.insert(key, manifest.clone());
        manifest
    }

    fn ancestors(&self, pid: u32) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut current = pid;
        while let Some(ppid) = self.procs.get(&current).and_then(|process| process.ppid) {
            if ppid == current || chain.contains(&ppid) || chain.len() >= 64 {
                break;
            }
            chain.push(ppid);
            current = ppid;
        }
        chain
    }

    fn descendants(&self, root: u32) -> Vec<u32> {
        let mut found = vec![root];
        let mut index = 0;
        while index < found.len() {
            if let Some(kids) = self.children.get(&found[index]) {
                for kid in kids {
                    if !found.contains(kid) {
                        found.push(*kid);
                    }
                }
            }
            index += 1;
        }
        found.sort_unstable();
        found
    }

    fn project_name(&self, id: Option<&str>) -> String {
        id.and_then(|id| self.projects.iter().find(|project| project.id == id))
            .map(|project| project.name.clone())
            .unwrap_or_default()
    }
}

/// Naming order: by base name, services with ports first, processes before
/// containers, then by PID or container ID.
fn draft_key(draft: &Draft<'_>) -> (String, bool, u8, String) {
    let portless = draft.ports.is_empty();
    match &draft.source {
        DraftSource::Process { main, .. } => {
            (draft.base_name.clone(), portless, 0, format!("{main:010}"))
        }
        DraftSource::Container { id } => (draft.base_name.clone(), portless, 1, id.clone()),
    }
}

fn stem_of(process: &RawProcess) -> String {
    process
        .exe
        .as_ref()
        .map(|exe| exe_stem(&exe.to_string_lossy()))
        .unwrap_or_else(|| exe_stem(&process.name))
}

fn is_wrapper(process: &RawProcess) -> bool {
    let stem = stem_of(process);
    let args = || process.argv.iter().skip(1);
    // npm and yarn rename themselves ("npm run dev"), so look at the name and the
    // first word of argv[0] as well as the executable.
    let title = process
        .argv
        .first()
        .and_then(|first| first.split_whitespace().next())
        .map(exe_stem)
        .unwrap_or_default();
    [
        stem.as_str(),
        exe_stem(&process.name).as_str(),
        title.as_str(),
    ]
    .iter()
    .any(|name| WRAPPER_EXES.contains(name))
        || args().any(|arg| {
            // Yarn 2+ runs as `node .yarn/releases/yarn-4.5.1.cjs`.
            let script = script_stem(arg);
            WRAPPER_SCRIPTS.contains(&script.as_str()) || script.starts_with("yarn-")
        })
        || (SHELLS.contains(&stem.as_str())
            && args().any(|arg| {
                arg.eq_ignore_ascii_case("/c")
                    || arg.eq_ignore_ascii_case("-command")
                    // `-c`, also in a cluster such as `bash -lc` or `sh -ec`.
                    || arg.strip_prefix('-').is_some_and(|flags| {
                        flags.contains('c')
                            && flags.len() <= 4
                            && flags.bytes().all(|byte| byte.is_ascii_lowercase())
                    })
            }))
        || (stem == "java" && args().any(|arg| arg.contains(MAVEN_LAUNCHER)))
}

fn container_for_port(
    listener: &RawListener,
    containers: &[(&RawContainer, Option<Place>, Option<Match<'_>>)],
) -> Option<String> {
    let addr = canonical_ip(listener.addr);
    let mut best: Option<(u8, &str)> = None;
    for (container, _, _) in containers {
        if container.state != "running" {
            continue;
        }
        for binding in &container.ports {
            if binding.host_port != Some(listener.port) {
                continue;
            }
            let score = match binding.host_ip.map(canonical_ip) {
                Some(ip) if ip == addr => 2,
                Some(ip) if ip.is_unspecified() => 1,
                None => 1,
                Some(_) => continue,
            };
            if best.is_none_or(|(current, _)| score > current) {
                best = Some((score, container.id.as_str()));
            }
        }
    }
    best.map(|(_, id)| id.to_string())
}

fn container_service_name(container: &RawContainer) -> String {
    match container.labels.get(&format!("{COMPOSE}service")) {
        Some(service) => match container
            .labels
            .get(&format!("{COMPOSE}container-number"))
            .and_then(|number| number.parse::<u32>().ok())
        {
            Some(number) if number > 1 => format!("{service}-{number}"),
            _ => service.clone(),
        },
        None => container.name.clone(),
    }
}

fn compose_of(container: &RawContainer) -> Option<Compose> {
    let label = |key: &str| container.labels.get(&format!("{COMPOSE}{key}"));
    Some(Compose {
        project: label("project")?.clone(),
        service: label("service")?.clone(),
        working_dir: label("project.working_dir").map(PathBuf::from),
        config_files: label("project.config_files")
            .map(|files| files.split(',').map(PathBuf::from).collect())
            .unwrap_or_default(),
        config_hash: label("config-hash").cloned(),
        oneoff: label("oneoff").is_some_and(|value| value.eq_ignore_ascii_case("true")),
        number: label("container-number").and_then(|number| number.parse().ok()),
    })
}

/// Last folder name, for paths of any OS: `C:\\work\\shop\\` → `shop`; a bare
/// repository's `shop.git` reads `shop`.
fn folder_name(path: &Path) -> String {
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches(['/', '\\']);
    let name = hm_core::names::basename(trimmed);
    let name = name
        .strip_suffix(".git")
        .filter(|stem| !stem.is_empty())
        .unwrap_or(name);
    if name.is_empty() {
        text.into_owned()
    } else {
        name.to_string()
    }
}

/// `Google Chrome` → `google-chrome`.
fn slug(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

fn short_id(id: &str) -> &str {
    &id[..id.len().min(12)]
}
