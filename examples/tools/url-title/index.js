// url_title: GET the page through harness.http (only hosts in manifest.json "network" work)
// and return the text of its <title>.

function decodeEntities(text) {
  const named = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' };
  return text.replace(/&(#x?[0-9a-f]+|[a-z]+);/gi, (whole, body) => {
    if (body[0] === '#') {
      const code = body[1].toLowerCase() === 'x' ? parseInt(body.slice(2), 16) : parseInt(body.slice(1), 10);
      return Number.isFinite(code) ? String.fromCodePoint(code) : whole;
    }
    return named[body.toLowerCase()] ?? whole;
  });
}

function run(args) {
  const page = harness.http.get(args.url);
  if (page.status >= 400) {
    throw new Error('the server answered HTTP ' + page.status);
  }
  const match = /<title[^>]*>([\s\S]*?)<\/title\s*>/i.exec(page.body);
  if (!match) {
    return 'No <title> found at ' + page.url;
  }
  return decodeEntities(match[1]).replace(/\s+/g, ' ').trim();
}
