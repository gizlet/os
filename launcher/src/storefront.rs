//! The Store pages' state: what gizlet.io lists for this device, the gizlet
//! open, a search, what is being installed, and the art on its way. Every
//! request runs on a worker thread through [`Store`]; [`Storefront::poll`]
//! takes in what they finished. Nobody signs in. The lists and search show
//! apps; a short link or GitHub address opens any gizlet, apps and services
//! alike. Pages in `ui::store` only draw this.

use crate::input::Action;
use gizlet_launcher::apps::{Kind, Library};
use gizlet_launcher::store::{Detail, Error, Featured, Origin, Progress, Store, Summary, UpdateStatus};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

/// Art downloads at once.
const ART_AT_ONCE: usize = 4;
/// The home lists are read again when the Store opens this long after.
const HOME_FRESH: f64 = 300.0;

pub enum Load<T> {
    Loading,
    Ready(T),
    Failed(Error),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(t) => Some(t),
            _ => None,
        }
    }
}

/// The most cards one list shows: as many as fit across the screen.
pub const SHELF_SLOTS: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// The large cards at the top: covers, names and descriptions.
    Hero,
    /// Installed gizlets with an update, after an "Update all" button.
    Updates,
    /// A list from gizlet.io, as icons.
    Shelf,
}

pub struct Row {
    pub kind: RowKind,
    pub title: String,
    pub titles: BTreeMap<String, String>,
    pub cards: Vec<Summary>,
}

impl Row {
    /// Places focus can be in: an Updates row starts with "Update all".
    pub fn slots(&self) -> usize {
        self.cards.len() + usize::from(self.kind == RowKind::Updates)
    }

    /// The card in slot `col`; None for "Update all".
    pub fn card(&self, col: usize) -> Option<&Summary> {
        match self.kind {
            RowKind::Updates => col.checked_sub(1).and_then(|i| self.cards.get(i)),
            _ => self.cards.get(col),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailFocus {
    Actions(usize),
    Shots(usize),
    About,
}

pub struct DetailView {
    pub repo: String,
    /// What a list showed of it, drawn until its page arrives.
    pub preview: Option<Summary>,
    pub data: Load<Detail>,
    pub focus: DetailFocus,
    /// The screenshot shown full screen.
    pub viewer: Option<usize>,
}

impl DetailView {
    pub fn summary(&self) -> Option<&Summary> {
        self.data.ready().map(|d| &d.summary).or(self.preview.as_ref())
    }
}

pub struct SearchView {
    pub query: String,
    pub results: Option<Load<Vec<Summary>>>,
    pub focus: usize,
}

/// What the gizlet page offers, by what is installed and what the Store does.
#[derive(Clone, Debug, PartialEq)]
pub enum Offer {
    Install,
    Update(String),
    Open,
    Manage,
    Uninstall,
    /// Installing it now, or waiting for another install first.
    Busy(Option<Progress>),
    Queued,
}

#[derive(Clone)]
enum JobKind {
    /// The repository, and the kind the person agreed to install.
    Install(String, Kind),
    Upgrade(Box<UpdateStatus>),
    Remove(String),
}

#[derive(Clone)]
pub struct Job {
    pub repo: String,
    pub id: String,
    pub name: String,
    kind: JobKind,
    pub progress: Option<Progress>,
}

impl Job {
    pub fn removing(&self) -> bool {
        matches!(self.kind, JobKind::Remove(_))
    }
}

pub enum Art {
    Loading,
    Ready(PathBuf),
    Failed,
}

/// What a worker finished.
enum Done {
    Home(Result<Featured, Error>),
    Updates(Result<Vec<UpdateStatus>, Error>),
    Detail(String, Result<Box<Detail>, Error>),
    Search(String, Result<Vec<Summary>, Error>),
    /// A link looked up: which lookup, and the repository it leads to.
    Resolved(u64, Result<String, Error>),
    Art(String, Result<PathBuf, Error>),
    Progress(Progress),
    Job(Result<(String, Kind, String), Error>),
}

/// What the launcher should do or say about what finished.
pub enum Note {
    Installed { name: String, kind: Kind, updated: bool },
    Removed(String),
    Failed(Error),
    /// A link led to this repository: open its page.
    Open(String),
}

pub struct Storefront {
    store: Store,
    prefix: PathBuf,
    send: Sender<Done>,
    work: Receiver<Done>,
    pub home: Load<Featured>,
    /// The lists shown are the ones saved last time, as the Store cannot be reached.
    pub saved: bool,
    home_asked: f64,
    /// The lists are being read: one request at a time writes the saved copy.
    home_pending: bool,
    pub updates: Vec<UpdateStatus>,
    pub origins: BTreeMap<String, Origin>,
    pub rows: Vec<Row>,
    pub row: usize,
    pub cols: Vec<usize>,
    /// The list shown under the hero card (a row past it): L and R go through them.
    pub tab: usize,
    pub detail: Option<DetailView>,
    pub search: Option<SearchView>,
    /// The link lookup waited for; an older one finishing is dropped.
    pub resolving: Option<u64>,
    links: u64,
    pub job: Option<Job>,
    pub queue: VecDeque<Job>,
    pub art: HashMap<String, Art>,
    art_queue: VecDeque<String>,
    art_running: usize,
}

impl Storefront {
    /// The Store of the library at `prefix`, which its workers open again.
    pub fn new(library: &Library, prefix: PathBuf) -> Storefront {
        Storefront::with_store(Store::new(library), prefix)
    }

    pub fn with_store(store: Store, prefix: PathBuf) -> Storefront {
        let (send, work) = mpsc::channel();
        Storefront {
            store,
            prefix,
            send,
            work,
            home: Load::Loading,
            saved: false,
            home_asked: f64::NEG_INFINITY,
            home_pending: false,
            updates: Vec::new(),
            origins: BTreeMap::new(),
            rows: Vec::new(),
            row: 0,
            cols: Vec::new(),
            tab: 0,
            detail: None,
            search: None,
            resolving: None,
            links: 0,
            job: None,
            queue: VecDeque::new(),
            art: HashMap::new(),
            art_queue: VecDeque::new(),
            art_running: 0,
        }
    }

    fn spawn(&self, work: impl FnOnce(&Store) -> Done + Send + 'static) {
        let (store, send) = (self.store.clone(), self.send.clone());
        std::thread::spawn(move || {
            let _ = send.send(work(&store));
        });
    }

    /// The Store opens: the lists, unless read a moment ago, and the updates.
    pub fn enter(&mut self, now: f64) {
        self.origins = self.store.origins();
        let stale = now - self.home_asked >= HOME_FRESH || !matches!(self.home, Load::Ready(_)) || self.saved;
        if stale {
            self.reload(now);
        }
        self.check_updates();
        // Art that failed last time, offline say, is asked for again.
        self.want_shown();
    }

    /// Reads the lists again; the saved ones show meanwhile, as when offline.
    pub fn reload(&mut self, now: f64) {
        if self.home_pending {
            return;
        }
        self.home_pending = true;
        self.home_asked = now;
        if self.rows.is_empty() {
            if let Some(saved) = self.store.featured_saved() {
                self.home = Load::Ready(saved);
                self.saved = true;
                self.rebuild();
            } else {
                self.home = Load::Loading;
            }
        }
        self.spawn(|store| Done::Home(store.featured()));
    }

    fn check_updates(&self) {
        let prefix = self.prefix.clone();
        self.spawn(move |store| Done::Updates(store.updates(&Library::new(prefix))));
    }

    pub fn loading(&self) -> bool {
        matches!(self.home, Load::Loading)
            || self.resolving.is_some()
            || self.job.is_some()
            || self.art_running > 0
            || self.detail.as_ref().is_some_and(|d| matches!(d.data, Load::Loading))
            || self.search.as_ref().is_some_and(|s| matches!(s.results, Some(Load::Loading)))
    }

    /// Takes in what the workers finished. Returns what to tell, and whether anything changed.
    pub fn poll(&mut self) -> (Vec<Note>, bool) {
        let mut notes = Vec::new();
        let mut changed = false;
        while let Ok(done) = self.work.try_recv() {
            changed = true;
            match done {
                Done::Home(Ok(featured)) => {
                    self.home_pending = false;
                    self.home = Load::Ready(featured);
                    self.saved = false;
                    self.rebuild();
                }
                Done::Home(Err(error)) => {
                    self.home_pending = false;
                    // The saved lists stay; without them, the page says why.
                    if !matches!(self.home, Load::Ready(_)) {
                        self.home = Load::Failed(error);
                    }
                }
                Done::Updates(Ok(updates)) => {
                    self.updates = updates;
                    self.rebuild();
                }
                Done::Updates(Err(_)) => {}
                Done::Detail(repo, result) => {
                    if let Some(detail) = self.detail.as_mut().filter(|d| d.repo == repo) {
                        detail.data = match result {
                            Ok(data) => Load::Ready(*data),
                            Err(error) => Load::Failed(error),
                        };
                        self.want_detail_art();
                    }
                }
                Done::Search(query, result) => {
                    if let Some(search) = self.search.as_mut().filter(|s| s.query == query) {
                        search.focus = 0;
                        search.results = Some(match result {
                            Ok(found) => Load::Ready(found),
                            Err(error) => Load::Failed(error),
                        });
                        let icons: Vec<String> = search.results.as_ref().and_then(Load::ready).into_iter().flatten().filter_map(|g| g.device_icon_url.clone()).collect();
                        for url in icons {
                            self.want(&url);
                        }
                    }
                }
                // Only the link still waited for: opening another page meanwhile drops it.
                Done::Resolved(seq, result) if self.resolving == Some(seq) => {
                    self.resolving = None;
                    notes.push(match result {
                        Ok(repo) => Note::Open(repo),
                        Err(error) => Note::Failed(error),
                    });
                }
                Done::Resolved(..) => {}
                Done::Art(url, result) => {
                    self.art_running -= 1;
                    // Art this evicted from the disk cache is fetched again when a page next asks for it.
                    self.art.insert(url, result.map_or(Art::Failed, Art::Ready));
                }
                Done::Progress(progress) => {
                    if let Some(job) = self.job.as_mut() {
                        job.progress = Some(progress);
                    }
                }
                Done::Job(result) => {
                    let Some(job) = self.job.take() else { continue };
                    let done = result.is_ok();
                    notes.push(match (result, &job.kind) {
                        (Ok(_), JobKind::Remove(_)) => Note::Removed(job.name),
                        (Ok((_, kind, _)), JobKind::Upgrade(_)) => Note::Installed { name: job.name, kind, updated: true },
                        (Ok((_, kind, _)), JobKind::Install(..)) => Note::Installed { name: job.name, kind, updated: false },
                        (Err(error), _) => Note::Failed(error),
                    });
                    self.origins = self.store.origins();
                    // A failed update stays offered, to try again.
                    if done {
                        self.updates.retain(|u| u.id != job.id);
                    }
                    self.rebuild();
                    if self.queue.is_empty() {
                        self.check_updates();
                    }
                    self.next_job();
                }
            }
        }
        self.pump_art();
        (notes, changed)
    }

    /// The rows of the home page: the hero cards, updates, then the lists.
    fn rebuild(&mut self) {
        let focused = self.focused().map(|g| g.repo.clone());
        let focused_row = self.rows.get(self.row).map(|r| r.kind);
        let shown = self.rows.get(self.tab).map(|r| (r.kind, r.title.clone()));
        self.rows.clear();
        if let Some(featured) = self.home.ready() {
            if !featured.hero.is_empty() {
                self.rows.push(Row { kind: RowKind::Hero, title: String::new(), titles: BTreeMap::new(), cards: featured.hero.clone() });
            }
        }
        // As many as one row holds; "Update all" covers updates past them, search the rest.
        let updates: Vec<Summary> = self.updates.iter().filter(|u| u.has_update()).take(SHELF_SLOTS - 1).map(update_card).collect();
        if !updates.is_empty() {
            self.rows.push(Row { kind: RowKind::Updates, title: String::new(), titles: BTreeMap::new(), cards: updates });
        }
        if let Some(featured) = self.home.ready() {
            for section in featured.sections.iter().filter(|s| !s.gizlets.is_empty()) {
                self.rows.push(Row { kind: RowKind::Shelf, title: section.title.clone(), titles: section.titles.clone(), cards: section.gizlets.iter().take(SHELF_SLOTS).cloned().collect() });
            }
        }
        self.cols = vec![0; self.rows.len()];
        // Focus stays on the same gizlet, or the same kind of row.
        let found = focused.as_ref().and_then(|repo| {
            self.rows.iter().enumerate().find_map(|(r, row)| (0..row.slots()).find(|&c| row.card(c).is_some_and(|g| &g.repo == repo)).map(|c| (r, c)))
        });
        if let Some((r, c)) = found {
            self.row = r;
            self.cols[r] = c;
        } else {
            self.row = focused_row.and_then(|kind| self.rows.iter().position(|r| r.kind == kind)).unwrap_or(0).min(self.rows.len().saturating_sub(1));
        }
        // The same list stays shown: the focused one, else the one shown before, else the first.
        let lists = self.lists();
        self.tab = if lists.contains(&self.row) {
            self.row
        } else {
            let before = shown.and_then(|(kind, title)| lists.clone().find(|&r| self.rows[r].kind == kind && self.rows[r].title == title));
            before.unwrap_or(lists.start)
        };
        self.want_shown();
    }

    /// Asks for the art of everything shown: the home page, the page open and search results.
    fn want_shown(&mut self) {
        let mut wanted: Vec<String> = Vec::new();
        for row in &self.rows {
            for g in &row.cards {
                wanted.extend(g.device_icon_url.clone());
                if row.kind == RowKind::Hero {
                    wanted.extend(g.device_cover_url.clone());
                }
            }
        }
        if let Some(found) = self.search.as_ref().and_then(|s| s.results.as_ref()).and_then(Load::ready) {
            wanted.extend(found.iter().filter_map(|g| g.device_icon_url.clone()));
        }
        for url in wanted {
            self.want(&url);
        }
        self.want_detail_art();
    }

    /// The card focused on the home page.
    pub fn focused(&self) -> Option<&Summary> {
        self.rows.get(self.row).and_then(|row| row.card(self.cols.get(self.row).copied().unwrap_or(0)))
    }

    /// "Update all" is focused.
    pub fn update_all_focused(&self) -> bool {
        self.rows.get(self.row).is_some_and(|row| row.kind == RowKind::Updates && self.cols[self.row] == 0)
    }

    /// The rows shown one at a time under the hero card: updates, then the lists.
    pub fn lists(&self) -> std::ops::Range<usize> {
        let first = usize::from(self.rows.first().is_some_and(|r| r.kind == RowKind::Hero));
        first..self.rows.len().max(first)
    }

    /// The home page fits the screen: the hero card above, one list below it.
    /// Up and Down move between them, L and R change the list.
    pub fn move_home(&mut self, action: Action) {
        if self.rows.is_empty() {
            return;
        }
        let lists = self.lists();
        let slots = self.rows[self.row].slots();
        let col = &mut self.cols[self.row];
        match action {
            Action::Left => *col = col.saturating_sub(1),
            Action::Right => *col = (*col + 1).min(slots.saturating_sub(1)),
            Action::Up if lists.start > 0 => self.row = 0,
            Action::Down if !lists.is_empty() => self.row = self.tab,
            Action::L | Action::R if !lists.is_empty() => {
                self.tab = if action == Action::L { self.tab.saturating_sub(1).max(lists.start) } else { (self.tab + 1).min(lists.end - 1) };
                self.row = self.tab;
            }
            _ => {}
        }
    }

    /// Asks for art not on hand: never asked for, failed before (offline, say), or evicted
    /// from the disk cache since.
    pub fn want(&mut self, url: &str) {
        let wanted = match self.art.get(url) {
            None | Some(Art::Failed) => true,
            Some(Art::Ready(path)) => !path.exists(),
            Some(Art::Loading) => false,
        };
        if wanted {
            self.art.insert(url.to_string(), Art::Loading);
            self.art_queue.push_back(url.to_string());
        }
    }

    fn pump_art(&mut self) {
        while self.art_running < ART_AT_ONCE {
            let Some(url) = self.art_queue.pop_front() else { break };
            self.art_running += 1;
            self.spawn(move |store| {
                let got = store.image(&url);
                Done::Art(url, got)
            });
        }
    }

    /// The art at `url` on the card, once downloaded.
    pub fn art_path(&self, url: Option<&str>) -> Option<&PathBuf> {
        match self.art.get(url?) {
            Some(Art::Ready(path)) => Some(path),
            _ => None,
        }
    }

    /// Opens a gizlet's page; `preview` is what a list showed of it.
    pub fn open(&mut self, repo: &str, preview: Option<Summary>) {
        self.resolving = None;
        self.detail = Some(DetailView { repo: repo.to_string(), preview, data: Load::Loading, focus: DetailFocus::Actions(0), viewer: None });
        self.want_detail_art();
        let repo = repo.to_string();
        self.spawn(move |store| {
            let got = store.detail(&repo).map(Box::new);
            Done::Detail(repo, got)
        });
    }

    fn want_detail_art(&mut self) {
        let Some(detail) = &self.detail else { return };
        let mut urls: Vec<String> = Vec::new();
        if let Some(s) = detail.summary() {
            urls.extend(s.device_icon_url.clone());
            urls.extend(s.device_cover_url.clone());
        }
        if let Some(d) = detail.data.ready() {
            urls.extend(d.device_screenshots.iter().cloned());
        }
        for url in urls {
            self.want(&url);
        }
    }

    /// What the open page offers, given what kind of gizlet with its ID is installed.
    pub fn offers(&self, installed: Option<Kind>) -> Vec<Offer> {
        let Some(detail) = &self.detail else { return Vec::new() };
        let Some(summary) = detail.summary() else { return Vec::new() };
        if let Some(job) = self.job.as_ref().filter(|j| j.repo == detail.repo) {
            return vec![if job.removing() { Offer::Busy(None) } else { Offer::Busy(job.progress) }];
        }
        if self.queue.iter().any(|j| j.repo == detail.repo) {
            return vec![Offer::Queued];
        }
        let Some(kind) = installed else {
            let available = detail.data.ready().is_none_or(|d| d.package_here().is_some());
            return if available { vec![Offer::Install] } else { Vec::new() };
        };
        let mut offers = Vec::new();
        let from_here = self.origins.get(&summary.id).is_some_and(|o| o.repo.eq_ignore_ascii_case(&detail.repo));
        if let Some(update) = self.updates.iter().find(|u| u.id == summary.id && u.has_update()).filter(|_| from_here) {
            offers.push(Offer::Update(update.version.clone().unwrap_or_default()));
        }
        offers.push(if kind == Kind::Service { Offer::Manage } else { Offer::Open });
        offers.push(Offer::Uninstall);
        offers
    }

    /// The gizlet on the open page is installed from somewhere else than its repository.
    pub fn other_source(&self, installed: Option<Kind>) -> bool {
        let (Some(detail), Some(_)) = (&self.detail, installed) else { return false };
        detail.summary().is_some_and(|s| self.origins.get(&s.id).is_none_or(|o| !o.repo.eq_ignore_ascii_case(&detail.repo)))
    }

    /// Keeps focus on a button the page still has, after an install or removal changes them.
    pub fn clamp_detail(&mut self, offers: usize) {
        if let Some(DetailView { focus: DetailFocus::Actions(i), .. }) = self.detail.as_mut() {
            *i = (*i).min(offers.saturating_sub(1));
        }
    }

    pub fn move_detail(&mut self, action: Action, offers: usize) {
        let Some(detail) = self.detail.as_mut() else { return };
        let shots = detail.data.ready().map_or(0, |d| d.device_screenshots.len());
        detail.focus = match (detail.focus, action) {
            (DetailFocus::Actions(i), Action::Left) => DetailFocus::Actions(i.saturating_sub(1)),
            (DetailFocus::Actions(i), Action::Right) => DetailFocus::Actions((i + 1).min(offers.saturating_sub(1))),
            (DetailFocus::Actions(_), Action::Down) if shots > 0 => DetailFocus::Shots(0),
            (DetailFocus::Actions(_), Action::Down) => DetailFocus::About,
            (DetailFocus::Shots(i), Action::Left) => DetailFocus::Shots(i.saturating_sub(1)),
            (DetailFocus::Shots(i), Action::Right) => DetailFocus::Shots((i + 1).min(shots - 1)),
            (DetailFocus::Shots(_), Action::Up) => DetailFocus::Actions(0),
            (DetailFocus::Shots(_), Action::Down) => DetailFocus::About,
            (DetailFocus::About, Action::Up) if shots > 0 => DetailFocus::Shots(0),
            (DetailFocus::About, Action::Up) => DetailFocus::Actions(0),
            (focus, _) => focus,
        };
        if let DetailFocus::Actions(i) = detail.focus {
            detail.focus = DetailFocus::Actions(i.min(offers.saturating_sub(1)));
        }
    }

    pub fn search(&mut self, query: String) {
        let query = query.trim().to_string();
        self.search = Some(SearchView { query: query.clone(), results: Some(Load::Loading), focus: 0 });
        self.spawn(move |store| {
            let got = store.search(&query);
            Done::Search(query, got)
        });
    }

    pub fn resolve(&mut self, link: String) {
        self.links += 1;
        let seq = self.links;
        self.resolving = Some(seq);
        self.spawn(move |store| Done::Resolved(seq, store.resolve(&link)));
    }

    /// Installs the gizlet on the open page, after what is installing now.
    pub fn install(&mut self, repo: &str, id: &str, name: &str, kind: Kind) {
        self.enqueue(Job { repo: repo.into(), id: id.into(), name: name.into(), kind: JobKind::Install(repo.into(), kind), progress: None });
    }

    /// Something is installing, updating or removing, or waits to.
    pub fn busy(&self) -> bool {
        self.job.is_some() || !self.queue.is_empty()
    }

    pub fn upgrade(&mut self, update: &UpdateStatus, name: &str) {
        let job = Job { repo: update.repo.clone(), id: update.id.clone(), name: name.into(), kind: JobKind::Upgrade(Box::new(update.clone())), progress: None };
        self.enqueue(job);
    }

    pub fn remove(&mut self, repo: &str, id: &str, name: &str) {
        self.enqueue(Job { repo: repo.into(), id: id.into(), name: name.into(), kind: JobKind::Remove(id.into()), progress: None });
    }

    /// Queues every update not queued yet; how many were.
    pub fn upgrade_all(&mut self, lang: &str) -> usize {
        let pending: Vec<UpdateStatus> = self.updates.iter().filter(|u| u.has_update()).cloned().collect();
        let mut queued = 0;
        for update in pending {
            if self.job.as_ref().is_some_and(|j| j.id == update.id) || self.queue.iter().any(|j| j.id == update.id) {
                continue;
            }
            let name = update.name_for(lang).to_string();
            self.upgrade(&update, &name);
            queued += 1;
        }
        queued
    }

    fn enqueue(&mut self, job: Job) {
        if self.job.as_ref().is_some_and(|j| j.id == job.id) || self.queue.iter().any(|j| j.id == job.id) {
            return;
        }
        self.queue.push_back(job);
        if self.job.is_none() {
            self.next_job();
        }
    }

    fn next_job(&mut self) {
        let Some(job) = self.queue.pop_front() else { return };
        let kind = job.kind.clone();
        let expected = job.id.clone();
        self.job = Some(job);
        let send = self.send.clone();
        let prefix = self.prefix.clone();
        self.spawn(move |store| {
            let library = Library::new(prefix);
            let mut progress = |p: Progress| {
                let _ = send.send(Done::Progress(p));
            };
            let finished = match kind {
                JobKind::Install(repo, kind) => store.install_expecting(&library, &repo, &expected, kind, &mut progress),
                JobKind::Upgrade(update) => store.upgrade(&library, &update, &mut progress),
                JobKind::Remove(id) => {
                    return Done::Job(store.uninstall(&library, &id).map(|kind| (id, kind, String::new())));
                }
            };
            Done::Job(finished.map(|app| (app.manifest.id.clone(), app.manifest.kind(), app.manifest.version.clone())))
        });
    }

    /// The update offered for the installed gizlet `id`, if any.
    pub fn update_for(&self, id: &str) -> Option<&UpdateStatus> {
        self.updates.iter().find(|u| u.id == id && u.has_update())
    }
}

/// An update as a card on the home page.
fn update_card(update: &UpdateStatus) -> Summary {
    let card = serde_json::json!({
        "repo": update.repo,
        "id": update.id,
        "name": update.name.clone().unwrap_or_else(|| update.id.clone()),
        "names": update.names,
        "kind": update.kind.clone().unwrap_or_else(|| "app".into()),
        "color": update.color,
        "device_icon_url": update.device_icon_url,
        "version": update.version,
    });
    serde_json::from_value(card).expect("an update makes a card")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(repo: &str) -> Summary {
        serde_json::from_value(serde_json::json!({ "repo": repo, "id": format!("dev.{repo}"), "name": repo })).unwrap()
    }

    fn front() -> Storefront {
        let dir = std::env::temp_dir().join(format!("gizlet-storefront-{}", std::process::id()));
        // Nothing listens there: the pages' requests fail without reaching gizlet.io.
        Storefront::with_store(Store::with_api(&Library::new(dir.clone()), "http://127.0.0.1:9"), dir)
    }

    #[test]
    fn rows_follow_the_lists_and_keep_focus_on_the_same_gizlet() {
        let mut s = front();
        s.home = Load::Ready(serde_json::from_value(serde_json::json!({
            "hero": [{ "repo": "h", "id": "dev.h", "name": "H" }],
            "sections": [{ "id": "new", "title": "New", "gizlets": [{ "repo": "a", "id": "dev.a", "name": "A" }, { "repo": "b", "id": "dev.b", "name": "B" }] }],
        })).unwrap());
        s.rebuild();
        assert_eq!(s.rows.iter().map(|r| r.kind).collect::<Vec<_>>(), [RowKind::Hero, RowKind::Shelf]);
        s.move_home(Action::Down);
        s.move_home(Action::Right);
        assert_eq!(s.focused().unwrap().repo, "b");
        // An update arrives: a row is added above, and focus stays on b.
        s.updates = vec![serde_json::from_value(serde_json::json!({
            "repo": "a", "id": "dev.a", "installed": "1.0.0", "status": "update", "version": "1.1.0",
            "package": { "file": "a.gzl", "target": "t", "tier": "lite", "size": 1, "sha256": null, "url": null },
        })).unwrap()];
        s.rebuild();
        assert_eq!(s.rows[1].kind, RowKind::Updates);
        assert_eq!(s.rows[1].slots(), 2);
        assert_eq!(s.focused().unwrap().repo, "b");
        assert_eq!(s.tab, 2, "the list shown stays");
        // L shows the updates instead; Up goes to the hero card and Down back to that list.
        s.move_home(Action::L);
        assert!(s.update_all_focused());
        s.move_home(Action::Up);
        assert_eq!(s.focused().unwrap().repo, "h");
        s.move_home(Action::Down);
        s.move_home(Action::Right);
        assert_eq!(s.focused().unwrap().repo, "a");
        s.move_home(Action::R);
        s.move_home(Action::R);
        assert_eq!((s.tab, s.focused().unwrap().repo.as_str()), (2, "b"));
    }

    #[test]
    fn art_that_failed_or_was_evicted_is_asked_for_again() {
        let mut s = front();
        s.art.insert("u/failed".into(), Art::Failed);
        s.art.insert("u/evicted".into(), Art::Ready(PathBuf::from("/nonexistent/gizlet-art.png")));
        s.art.insert("u/loading".into(), Art::Loading);
        for url in ["u/failed", "u/evicted", "u/loading"] {
            s.want(url);
        }
        assert!(s.art.values().all(|a| matches!(a, Art::Loading)));
        assert_eq!(s.art_queue, ["u/failed", "u/evicted"]);
    }

    #[test]
    fn a_link_resolved_after_another_page_opened_is_dropped() {
        let mut s = front();
        s.resolving = Some(1);
        s.open("github.com/o/a", Some(card("a")));
        s.send.send(Done::Resolved(1, Ok("github.com/o/b".into()))).unwrap();
        let (notes, _) = s.poll();
        assert!(notes.is_empty());
        assert_eq!(s.detail.as_ref().unwrap().repo, "github.com/o/a");
    }

    #[test]
    fn only_the_last_link_entered_opens() {
        let mut s = front();
        s.resolving = Some(2);
        s.send.send(Done::Resolved(1, Ok("github.com/o/a".into()))).unwrap();
        let (notes, _) = s.poll();
        assert!(notes.is_empty(), "an older lookup is dropped");
        s.send.send(Done::Resolved(2, Ok("github.com/o/b".into()))).unwrap();
        let (notes, _) = s.poll();
        assert!(matches!(notes.as_slice(), [Note::Open(repo)] if repo == "github.com/o/b"));
        assert_eq!(s.resolving, None);
    }

    #[test]
    fn the_lists_are_read_once_at_a_time() {
        let mut s = front();
        s.reload(1.0);
        s.reload(2.0);
        assert_eq!(s.home_asked, 1.0, "a second read waits for the first");
        while s.home_pending {
            s.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        s.reload(3.0);
        assert_eq!(s.home_asked, 3.0);
    }

    #[test]
    fn a_queued_install_keeps_the_store_busy() {
        let mut s = front();
        assert!(!s.busy());
        s.install("github.com/o/a", "dev.a", "A", Kind::App);
        assert!(s.busy());
    }

    #[test]
    fn the_page_offers_what_fits_what_is_installed() {
        let mut s = front();
        s.open("github.com/o/a", Some(card("a")));
        assert_eq!(s.offers(None), [Offer::Install]);
        // Installed from a file: open it or remove it, but no update from here.
        assert_eq!(s.offers(Some(Kind::App)), [Offer::Open, Offer::Uninstall]);
        assert!(s.other_source(Some(Kind::App)));
        s.origins.insert("dev.a".into(), serde_json::from_value(serde_json::json!({ "repo": "github.com/o/a", "version": "1.0.0", "tag": "v1.0.0", "sha256": "0" })).unwrap());
        s.updates = vec![serde_json::from_value(serde_json::json!({
            "repo": "github.com/o/a", "id": "dev.a", "installed": "1.0.0", "status": "update", "version": "1.1.0",
            "package": { "file": "a.gzl", "target": "t", "tier": "lite", "size": 1, "sha256": null, "url": null },
        })).unwrap()];
        assert_eq!(s.offers(Some(Kind::Service)), [Offer::Update("1.1.0".into()), Offer::Manage, Offer::Uninstall]);
        assert!(!s.other_source(Some(Kind::Service)));
        s.move_detail(Action::Right, 3);
        s.move_detail(Action::Right, 3);
        s.move_detail(Action::Right, 3);
        assert_eq!(s.detail.as_ref().unwrap().focus, DetailFocus::Actions(2));
        s.move_detail(Action::Down, 3);
        assert_eq!(s.detail.as_ref().unwrap().focus, DetailFocus::About);
    }
}
