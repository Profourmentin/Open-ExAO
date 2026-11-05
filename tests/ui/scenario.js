// Scénario de vérification de l'interface (optionnel : demande Node et Playwright).
// 1. cargo run --release -p openexao-app -- --no-open --port 8765   (noter l'adresse affichée)
// 2. node tests/ui/scenario.js <dossier de sortie> <adresse complète avec le jeton>
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

const S = process.argv[2];          // dossier de travail (captures, téléchargements)
const URL = process.argv[3];        // adresse complète avec jeton
let failures = 0;
const check = (ok, label, detail = '') => {
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${label}${ok ? '' : '  -> ' + detail}`);
  if (!ok) failures++;
};

(async () => {
  const browser = await chromium.launch();
  const context = await browser.newContext({ viewport: { width: 1280, height: 760 }, acceptDownloads: true });
  const page = await context.newPage();
  const problems = [];
  page.on('pageerror', (e) => problems.push('pageerror: ' + e.message));
  page.on('console', (m) => { if (m.type() === 'error') problems.push('console: ' + m.text()); });
  page.on('dialog', (d) => d.accept());

  const val = (c, r) => page.locator(`[data-key="c:${c}:${r}"]`).inputValue();
  const toastText = () => page.locator('#toasts').innerText();

  await page.goto(URL);
  await page.waitForSelector('td input.cell');
  await page.screenshot({ path: path.join(S, 'ui-1-initial.png') });

  check((await page.locator('#count').innerText()).startsWith('11 lignes × 4 colonnes'), 'compteur initial', await page.locator('#count').innerText());
  check((await val(2, 10)) === '4,905', 'y_th à t=1 vaut 4,905', await val(2, 10));
  check((await page.title()).includes('Open-ExAO'), 'titre de la page', await page.title());

  // 1. modification d'une valeur : recalcul des colonnes calculées
  await page.locator('[data-key="c:0:10"]').fill('2');
  await page.locator('[data-key="c:0:10"]').press('Enter');
  await page.waitForFunction(() => document.querySelector('[data-key="c:2:10"]').value === '19,62');
  check((await val(2, 10)) === '19,62', 'recalcul après modification de t');

  // 2. valeur invalide : refusée une seule fois, le champ garde la saisie pour la corriger,
  //    la table reste inchangée, et en quittant la cellule on revient à la valeur valide
  await page.locator('[data-key="c:0:10"]').fill('abc');
  await page.locator('[data-key="c:0:10"]').press('Enter');
  await page.waitForTimeout(300);
  const toast = await toastText();
  check(/nombre/i.test(toast), 'message clair pour une valeur invalide', toast);
  check((await val(2, 10)) === '19,62', 'la valeur invalide ne modifie pas les calculs', await val(2, 10));
  await page.locator('[data-key="c:0:9"]').click();
  await page.waitForTimeout(300);
  check((await val(0, 10)) === '2', 'en quittant la cellule, retour à la valeur valide', await val(0, 10));
  check((await page.locator('#toasts .toast').count()) === 1, 'un seul message d\'erreur pour un seul refus', String(await page.locator('#toasts .toast').count()));

  // 3. ajout d'une colonne calculée
  await page.click('#btn-add-calc');
  await page.fill('#dlg-name', 'v_th');
  await page.fill('#dlg-unit', 'm/s');
  await page.fill('#dlg-formula', '9.81*t');
  await page.click('#dlg-ok');
  await page.waitForSelector('[data-key="c:4:0"]');
  check((await val(4, 3)) === '2,943', 'colonne v_th = 9,81*t à t=0,3', await val(4, 3));
  check((await page.locator('#count').innerText()).includes('5 colonnes'), 'compteur après ajout de colonne');

  // 4. formule fausse : la colonne est créée (comme un #NOM? de tableur) et l'erreur est expliquée
  await page.click('#btn-add-calc');
  await page.fill('#dlg-name', 'faux');
  await page.fill('#dlg-formula', '9.81*tt');
  await page.click('#dlg-ok');
  await page.waitForSelector('[data-key="c:5:0"]');
  const ferr = await page.locator('.ferr').first().innerText();
  check(/tt/.test(ferr), 'formule avec nom inconnu : l\'erreur nomme le nom fautif', ferr);
  check((await page.locator('td.calc.err').count()) > 0, 'cellules de la colonne en erreur marquées');
  check((await page.locator('#count').innerText()).includes('6 colonnes'), 'la colonne fautive existe (6 colonnes)');
  await page.click('button[aria-label="Supprimer la colonne faux"]');
  await page.waitForFunction(() => document.querySelector('#count').textContent.includes('5 colonnes'));
  check(true, 'suppression de la colonne fautive');

  // 5. Entrée sur la dernière ligne : ajoute une ligne
  await page.locator('[data-key="c:0:10"]').focus();
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => document.querySelector('#count').textContent.startsWith('12 lignes'));
  check(true, 'Entrée sur la dernière ligne ajoute une ligne');

  await page.screenshot({ path: path.join(S, 'ui-2-edited.png') });

  // 6. exports
  const [csv] = await Promise.all([page.waitForEvent('download'), page.click('#btn-csv')]);
  const csvPath = path.join(S, 'export.csv');
  await csv.saveAs(csvPath);
  const csvText = fs.readFileSync(csvPath, 'utf8').replace(/^﻿/, '');
  check(csvText.split('\n')[0] === 't (s);y (m);y_th (m);ecart (m);v_th (m/s)', 'en-tête du CSV français', csvText.split('\n')[0]);
  check(/\n0,3;0,44;/.test(csvText), 'CSV français : séparateur « ; » et virgule décimale');

  const [exao] = await Promise.all([page.waitForEvent('download'), page.click('#btn-save')]);
  const exaoPath = path.join(S, 'projet.exao');
  await exao.saveAs(exaoPath);
  check(exao.suggestedFilename().endsWith('.exao'), 'nom du projet téléchargé', exao.suggestedFilename());

  // 7. nouveau projet puis réouverture du fichier téléchargé
  await page.click('#btn-new');
  await page.waitForFunction(() => !document.querySelector('[data-key="c:4:0"]'));
  await page.setInputFiles('#file', exaoPath);
  await page.waitForSelector('[data-key="c:4:0"]');
  check((await val(4, 3)) === '2,943', 'projet rouvert : la colonne calculée est recalculée', await val(4, 3));
  check((await page.locator('#title').inputValue()).startsWith('Chute libre'), 'projet rouvert : titre conservé', await page.locator('#title').inputValue());

  // 8. ouverture d'un CSV (français, avec BOM)
  await page.setInputFiles('#file', csvPath);
  await page.waitForTimeout(500);
  check((await page.locator('#count').innerText()).includes('5 colonnes'), 'CSV réimporté avec ses 5 colonnes', await page.locator('#count').innerText());

  check(problems.length === 0, 'aucune erreur dans la console du navigateur', problems.join(' | '));
  await browser.close();
  console.log(failures === 0 ? '\nTOUT EST OK' : `\n${failures} ÉCHEC(S)`);
  process.exit(failures === 0 ? 0 : 1);
})().catch((e) => { console.error('ERREUR DU SCÉNARIO:', e.message); process.exit(2); });
