// Dedicated "PWA" profile for Outlook — hand-rolled SSB via Firefox.
// Enables userChrome.css so we can strip the browser chrome.
user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);

// Behave like an app, not a browser.
user_pref("browser.tabs.warnOnClose", false);
user_pref("browser.sessionstore.resume_from_crash", false);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.startup.homepage_override.mstone", "ignore"); // no "what's new" page
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.warnOnQuit", false);

// Keep everything in one window. The tab strip is hidden, so links must
// NOT spawn hidden tabs: force target=_blank / window.open into the
// current tab (1) and apply that even to _blank links (restriction 0).
// The visible nav bar's Back button then returns you to the inbox.
user_pref("browser.link.open_newwindow", 1);
user_pref("browser.link.open_newwindow.restriction", 0);
