//! Application locale d'Open-ExAO : un tableur de mesures dans le navigateur,
//! servi depuis votre ordinateur, sans dépendance externe.
//!
//! ```text
//!  navigateur (ui.html : affichage seulement)
//!        │  JSON sur http://127.0.0.1:<port>   (jeton secret)
//!  server  ──►  session  ──►  openexao-core  (formules, CSV, projet)
//! ```
//!
//! - [`session`] : l'état (projet ouvert) et les commandes qui le modifient
//! - [`server`] : le serveur local et sa sécurité
//! - [`http`] : le strict nécessaire de HTTP/1.1
//!
//! Aucune règle de calcul ne vit ici ni dans la page : tout est dans le noyau.

pub mod http;
pub mod server;
pub mod session;

pub use server::Server;
pub use session::Session;
